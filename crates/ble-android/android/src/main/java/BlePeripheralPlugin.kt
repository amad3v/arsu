package io.github.amad3v.arsu.ble

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothGattServer
import android.bluetooth.BluetoothGattServerCallback
import android.bluetooth.BluetoothGattService
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.BluetoothStatusCodes
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.bluetooth.le.BluetoothLeAdvertiser
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Build
import android.os.ParcelUuid
import android.util.Base64
import app.tauri.PermissionState
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.util.UUID

// Arsu's GATT layout, as `vault_sync::gatt` defines it.
private val SERVICE_UUID: UUID = UUID.fromString("8a8f11c2-b37f-496e-97a9-b1ba6eb6ee78")
private val RX_UUID: UUID = UUID.fromString("d38b2aeb-d8f3-4c2d-bcda-8300b7fa5d42")
private val TX_UUID: UUID = UUID.fromString("45906ec1-7669-42a7-a423-266fd2995885")

/** The descriptor a central subscribes to notifications through. */
private val CLIENT_CONFIG_UUID: UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")

/** The smallest ATT MTU, which every link has until the central asks for more. */
private const val MIN_ATT_MTU = 23

/** The permissions advertising and serving take from Android 12 on ("Nearby devices"). */
private const val NEARBY = "nearby"

@InvokeArg
class StartArgs {
    var session: Long = 0

    /** An invitation's token, in base64, to advertise for pairing; none to advertise for sync. */
    var token: String? = null
    lateinit var events: Channel
}

@InvokeArg
class NotifyArgs {
    var session: Long = 0
    lateinit var chunk: String
}

@InvokeArg
class StopArgs {
    var session: Long = 0
}

/** A session: who it reports to, and the central it serves once one has written. */
private class Session(val id: Long, val events: Channel) {
    var central: BluetoothDevice? = null
}

/**
 * Arsu as a Bluetooth LE peripheral: the phone advertises,
 * and serves Arsu's GATT service the way a computer does, so that any
 * device finds and syncs with it the same way.
 *
 * The service is opened once and kept for the app's life, so that its
 * attribute handles never change: a central remembers them between
 * connections. Each session (`start`, `stop`) only advertises, and
 * serves the first central that writes to `rx`; writes from any other
 * device are refused until the next session. The last session's central,
 * if it stays connected, is refused until it leaves: a session that
 * failed ends while its central is still writing the rest of a message,
 * which would start the next session out of order and end it at once.
 * It is not disconnected: a session that succeeds ends with the last
 * message possibly still on its way to it. The central's chunks and
 * its leaving go to the session's channel; `notify` sends a chunk on
 * `tx`, and answers once Android has sent it, which paces the sender.
 *
 * Rejections carry a code the Rust side maps: off, unsupported, or failed.
 */
@SuppressLint("MissingPermission") // `ensurePermission` comes first, from Rust.
@TauriPlugin(
    permissions = [
        Permission(
            strings = [Manifest.permission.BLUETOOTH_ADVERTISE, Manifest.permission.BLUETOOTH_CONNECT],
            alias = NEARBY
        )
    ]
)
class BlePeripheralPlugin(private val activity: Activity) : Plugin(activity) {
    private val manager: BluetoothManager = activity.getSystemService(BluetoothManager::class.java)

    /** Guards everything below: Android calls the server back on its own threads. */
    private val lock = Any()
    private var server: BluetoothGattServer? = null
    private var tx: BluetoothGattCharacteristic? = null

    /** Whether Android has added Arsu's service to the server. */
    private var serviceReady = false

    /** Sessions waiting for the service to be added, to go on starting. */
    private val serviceWaiters = mutableListOf<(Boolean) -> Unit>()
    private var session: Session? = null
    /** The last session's central while it stays connected: refused until it leaves. */
    private var dismissed: String? = null
    private var advertiser: BluetoothLeAdvertiser? = null
    private var advertising: AdvertiseCallback? = null
    private var pendingNotify: Invoke? = null
    private val mtus = HashMap<String, Int>()
    private val subscribed = HashSet<String>()

    /**
     * Bluetooth turning off takes the server with it: forget it, so that
     * the next session opens it again, and end the session in progress.
     */
    private val bluetoothState = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            val state = intent.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR)
            if (state != BluetoothAdapter.STATE_TURNING_OFF && state != BluetoothAdapter.STATE_OFF) return
            synchronized(lock) {
                advertising = null
                failPendingNotify("Bluetooth turned off")
                session?.let { current ->
                    if (current.central != null) current.events.send(event(current.id, "left"))
                }
                session = null
                dismissed = null
                mtus.clear()
                subscribed.clear()
            }
            serviceOpened(false)
        }
    }

    init {
        val filter = IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            activity.registerReceiver(bluetoothState, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            activity.registerReceiver(bluetoothState, filter)
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        activity.unregisterReceiver(bluetoothState)
        synchronized(lock) {
            stopAdvertising()
            failPendingNotify("the app is closing")
            session = null
        }
        serviceOpened(false)
    }

    @Command
    fun ensurePermission(invoke: Invoke) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S ||
            getPermissionState(NEARBY) == PermissionState.GRANTED
        ) {
            invoke.resolve(granted(true))
        } else {
            requestPermissionForAlias(NEARBY, invoke, "nearbyAnswered")
        }
    }

    // Called by name, once the user has answered (`requestPermissionForAlias`).
    @PermissionCallback
    fun nearbyAnswered(invoke: Invoke) {
        invoke.resolve(granted(getPermissionState(NEARBY) == PermissionState.GRANTED))
    }

    private fun granted(granted: Boolean) = JSObject().apply { put("granted", granted) }

    @Command
    fun start(invoke: Invoke) {
        val args = invoke.parseArgs(StartArgs::class.java)
        val token = args.token?.let { Base64.decode(it, Base64.NO_WRAP) }
        val adapter = manager.adapter
        if (adapter == null || !adapter.isEnabled) {
            invoke.reject("Bluetooth is off", "off")
            return
        }
        if (!adapter.isMultipleAdvertisementSupported) {
            invoke.reject("this phone cannot advertise", "unsupported")
            return
        }
        val leAdvertiser = adapter.bluetoothLeAdvertiser
        if (leAdvertiser == null) {
            invoke.reject("Bluetooth is off", "off")
            return
        }
        synchronized(lock) {
            stopAdvertising()
            failPendingNotify("a new session started")
            session?.let { dismissIfConnected(it) }
            session = Session(args.session, args.events)
            advertiser = leAdvertiser
        }
        withService { ready ->
            if (ready) {
                advertise(leAdvertiser, args.session, token, invoke)
            } else {
                invoke.reject("Arsu's Bluetooth service could not be opened", "failed")
            }
        }
    }

    @Command
    fun notify(invoke: Invoke) {
        val args = invoke.parseArgs(NotifyArgs::class.java)
        val chunk = Base64.decode(args.chunk, Base64.NO_WRAP)
        val gattServer: BluetoothGattServer
        val central: BluetoothDevice
        val characteristic: BluetoothGattCharacteristic
        synchronized(lock) {
            val current = session
            if (current == null || current.id != args.session) {
                invoke.reject("this session has ended", "failed")
                return
            }
            val listening = current.central?.takeIf { it.address in subscribed }
            val openServer = server
            val notifier = tx
            if (listening == null || openServer == null || notifier == null) {
                invoke.reject("no central listens", "failed")
                return
            }
            if (pendingNotify != null) {
                invoke.reject("a notification is still on its way", "failed")
                return
            }
            pendingNotify = invoke
            gattServer = openServer
            central = listening
            characteristic = notifier
        }
        if (!sendNotification(gattServer, central, characteristic, chunk)) {
            synchronized(lock) {
                if (pendingNotify === invoke) pendingNotify = null
            }
            invoke.reject("Android refused the notification", "failed")
        }
    }

    @Command
    fun stop(invoke: Invoke) {
        val args = invoke.parseArgs(StopArgs::class.java)
        synchronized(lock) {
            // An earlier session's stop, arriving after the next one started, is not for it.
            session?.takeIf { it.id == args.session }?.let { ended ->
                stopAdvertising()
                failPendingNotify("the session has ended")
                dismissIfConnected(ended)
                session = null
            }
        }
        invoke.resolve()
    }

    /** Keeps the central `ended` served out while it stays connected. Holds [lock]. */
    private fun dismissIfConnected(ended: Session) {
        val central = ended.central ?: return
        if (manager.getConnectionState(central, BluetoothProfile.GATT_SERVER) == BluetoothProfile.STATE_CONNECTED) {
            dismissed = central.address
        }
    }

    /**
     * Runs `then` once Arsu's service is open: at once if it is, after
     * Android has added it otherwise (with every other session that asked
     * meanwhile). `then` learns whether it opened.
     */
    private fun withService(then: (Boolean) -> Unit) {
        val gattServer = synchronized(lock) {
            if (serviceReady) {
                null
            } else {
                serviceWaiters.add(then)
                // The first to wait opens it; the others wait for the same.
                if (server != null) return
                manager.openGattServer(activity, callback).also { server = it }
            }
        }
        if (gattServer == null) {
            if (synchronized(lock) { serviceReady }) then(true) else serviceOpened(false)
            return
        }
        val rx = BluetoothGattCharacteristic(
            RX_UUID,
            BluetoothGattCharacteristic.PROPERTY_WRITE,
            BluetoothGattCharacteristic.PERMISSION_WRITE
        )
        val notifier = BluetoothGattCharacteristic(
            TX_UUID,
            BluetoothGattCharacteristic.PROPERTY_NOTIFY,
            0
        )
        notifier.addDescriptor(
            BluetoothGattDescriptor(
                CLIENT_CONFIG_UUID,
                BluetoothGattDescriptor.PERMISSION_READ or BluetoothGattDescriptor.PERMISSION_WRITE
            )
        )
        val service = BluetoothGattService(SERVICE_UUID, BluetoothGattService.SERVICE_TYPE_PRIMARY)
        service.addCharacteristic(rx)
        service.addCharacteristic(notifier)
        synchronized(lock) { tx = notifier }
        if (!gattServer.addService(service)) serviceOpened(false)
    }

    /**
     * Tells every session waiting for the service whether it opened. One
     * that did not is closed, to open again on the next session.
     */
    private fun serviceOpened(opened: Boolean) {
        val waiters = synchronized(lock) {
            serviceReady = opened
            if (!opened) {
                server?.close()
                server = null
                tx = null
            }
            serviceWaiters.toList().also { serviceWaiters.clear() }
        }
        waiters.forEach { it(opened) }
    }

    private fun advertise(leAdvertiser: BluetoothLeAdvertiser, sessionId: Long, token: ByteArray?, invoke: Invoke) {
        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_MEDIUM)
            .setConnectable(true)
            .setTimeout(0)
            .build()
        // Each fits a legacy advertisement (31 bytes), as on a computer:
        // flags and the service's UUID (21 bytes), or flags and the token as the
        // service's data (29 bytes). No name, no transmit power.
        val data = AdvertiseData.Builder()
            .setIncludeDeviceName(false)
            .setIncludeTxPowerLevel(false)
            .apply {
                if (token == null) {
                    addServiceUuid(ParcelUuid(SERVICE_UUID))
                } else {
                    addServiceData(ParcelUuid(SERVICE_UUID), token)
                }
            }
            .build()
        val started = object : AdvertiseCallback() {
            override fun onStartSuccess(settingsInEffect: AdvertiseSettings) {
                invoke.resolve()
            }

            override fun onStartFailure(errorCode: Int) {
                synchronized(lock) {
                    if (advertising === this) advertising = null
                }
                val code = if (errorCode == ADVERTISE_FAILED_FEATURE_UNSUPPORTED) "unsupported" else "failed"
                invoke.reject("advertising failed (error $errorCode)", code)
            }
        }
        val current = synchronized(lock) {
            (session?.id == sessionId).also { if (it) advertising = started }
        }
        if (current) {
            leAdvertiser.startAdvertising(settings, data, started)
        } else {
            invoke.reject("the session has ended", "failed")
        }
    }

    /** Under the lock. */
    private fun stopAdvertising() {
        val callback = advertising ?: return
        advertising = null
        advertiser?.stopAdvertising(callback)
    }

    /** Under the lock. */
    private fun failPendingNotify(reason: String) {
        pendingNotify?.reject(reason, "failed")
        pendingNotify = null
    }

    private fun sendNotification(
        gattServer: BluetoothGattServer,
        central: BluetoothDevice,
        characteristic: BluetoothGattCharacteristic,
        chunk: ByteArray
    ): Boolean =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            gattServer.notifyCharacteristicChanged(central, characteristic, false, chunk) ==
                BluetoothStatusCodes.SUCCESS
        } else {
            // The only way before Android 13, which deprecates it for the call above.
            @Suppress("DEPRECATION")
            characteristic.value = chunk
            @Suppress("DEPRECATION")
            gattServer.notifyCharacteristicChanged(central, characteristic, false)
        }

    private fun answer(device: BluetoothDevice, requestId: Int, status: Int, value: ByteArray?) {
        synchronized(lock) { server }?.sendResponse(device, requestId, status, 0, value)
    }

    private val callback = object : BluetoothGattServerCallback() {
        override fun onServiceAdded(status: Int, service: BluetoothGattService) {
            serviceOpened(status == BluetoothGatt.GATT_SUCCESS)
        }

        override fun onMtuChanged(device: BluetoothDevice, mtu: Int) {
            synchronized(lock) { mtus[device.address] = mtu }
        }

        override fun onConnectionStateChange(device: BluetoothDevice, status: Int, newState: Int) {
            if (newState != BluetoothProfile.STATE_DISCONNECTED) return
            synchronized(lock) {
                mtus.remove(device.address)
                subscribed.remove(device.address)
                if (dismissed == device.address) dismissed = null
                val current = session
                if (current != null && current.central?.address == device.address) {
                    failPendingNotify("the central disconnected")
                    current.events.send(event(current.id, "left"))
                }
            }
        }

        override fun onCharacteristicWriteRequest(
            device: BluetoothDevice,
            requestId: Int,
            characteristic: BluetoothGattCharacteristic,
            preparedWrite: Boolean,
            responseNeeded: Boolean,
            offset: Int,
            value: ByteArray
        ) {
            val accepted = characteristic.uuid == RX_UUID && !preparedWrite && offset == 0 &&
                synchronized(lock) {
                    val current = session
                    val central = current?.central
                    when {
                        current == null -> false
                        device.address == dismissed -> false
                        central != null && central.address != device.address -> false
                        else -> {
                            current.central = device
                            // Answering only once the chunk is passed on is the flow
                            // control: the central sends the next one after the answer.
                            current.events.send(
                                event(current.id, "chunk").apply {
                                    put("chunk", Base64.encodeToString(value, Base64.NO_WRAP))
                                    put("mtu", mtus[device.address] ?: MIN_ATT_MTU)
                                    put("from", device.address)
                                }
                            )
                            true
                        }
                    }
                }
            if (responseNeeded) {
                val status = if (accepted) BluetoothGatt.GATT_SUCCESS else BluetoothGatt.GATT_WRITE_NOT_PERMITTED
                answer(device, requestId, status, null)
            }
        }

        override fun onDescriptorWriteRequest(
            device: BluetoothDevice,
            requestId: Int,
            descriptor: BluetoothGattDescriptor,
            preparedWrite: Boolean,
            responseNeeded: Boolean,
            offset: Int,
            value: ByteArray
        ) {
            val accepted = descriptor.uuid == CLIENT_CONFIG_UUID && !preparedWrite && offset == 0
            if (accepted) {
                synchronized(lock) {
                    if (value.contentEquals(BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE)) {
                        subscribed.add(device.address)
                    } else {
                        subscribed.remove(device.address)
                    }
                }
            }
            if (responseNeeded) {
                val status = if (accepted) BluetoothGatt.GATT_SUCCESS else BluetoothGatt.GATT_WRITE_NOT_PERMITTED
                answer(device, requestId, status, null)
            }
        }

        override fun onDescriptorReadRequest(
            device: BluetoothDevice,
            requestId: Int,
            offset: Int,
            descriptor: BluetoothGattDescriptor
        ) {
            if (descriptor.uuid != CLIENT_CONFIG_UUID) {
                answer(device, requestId, BluetoothGatt.GATT_READ_NOT_PERMITTED, null)
                return
            }
            val on = synchronized(lock) { device.address in subscribed }
            val value = if (on) {
                BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
            } else {
                BluetoothGattDescriptor.DISABLE_NOTIFICATION_VALUE
            }
            answer(device, requestId, BluetoothGatt.GATT_SUCCESS, value)
        }

        override fun onNotificationSent(device: BluetoothDevice, status: Int) {
            val pending = synchronized(lock) { pendingNotify.also { pendingNotify = null } }
            if (status == BluetoothGatt.GATT_SUCCESS) {
                pending?.resolve()
            } else {
                pending?.reject("the notification failed (status $status)", "failed")
            }
        }
    }

    private fun event(session: Long, kind: String) = JSObject().apply {
        put("session", session)
        put("kind", kind)
    }
}
