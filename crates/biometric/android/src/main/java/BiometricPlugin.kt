package io.github.amad3v.arsu.biometric

import android.app.Activity
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import android.util.Base64
import android.view.ViewTreeObserver
import androidx.appcompat.app.AppCompatActivity
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricManager.Authenticators.BIOMETRIC_STRONG
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.Lifecycle
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

private const val KEYSTORE = "AndroidKeyStore"
private const val KEY_ALIAS = "arsu-vault-unlock"
private const val TRANSFORMATION = "AES/GCM/NoPadding"
private const val TAG_BITS = 128

@InvokeArg
open class PromptArgs {
    lateinit var title: String
    lateinit var subtitle: String
    lateinit var cancel: String
}

@InvokeArg
class SealArgs : PromptArgs() {
    lateinit var data: String
}

@InvokeArg
class OpenArgs : PromptArgs() {
    lateinit var iv: String
    lateinit var ciphertext: String
}

/**
 * Seals a secret (Arsu's vault key) under an AES key held by Android
 * Keystore, which the secure hardware uses only after a strong biometric
 * check, through BiometricPrompt's CryptoObject: the cipher itself is
 * unlocked by the fingerprint or face, not a yes/no answer. Adding a
 * fingerprint or face to the phone invalidates the key.
 *
 * Rejections carry a code the Rust side maps: cancelled, lockout,
 * invalidated, unavailable, or failed.
 */
@TauriPlugin
class BiometricPlugin(private val activity: Activity) : Plugin(activity) {

    @Command
    fun status(invoke: Invoke) {
        val result = BiometricManager.from(activity).canAuthenticate(BIOMETRIC_STRONG)
        val ret = JSObject()
        ret.put("available", result == BiometricManager.BIOMETRIC_SUCCESS)
        ret.put(
            "reason",
            when (result) {
                BiometricManager.BIOMETRIC_SUCCESS -> null
                BiometricManager.BIOMETRIC_ERROR_NO_HARDWARE -> "no-hardware"
                BiometricManager.BIOMETRIC_ERROR_NONE_ENROLLED -> "not-enrolled"
                BiometricManager.BIOMETRIC_ERROR_HW_UNAVAILABLE -> "unavailable"
                BiometricManager.BIOMETRIC_ERROR_SECURITY_UPDATE_REQUIRED -> "update-required"
                else -> "unsupported"
            }
        )
        ret.put("keyExists", keyStore().containsAlias(KEY_ALIAS))
        invoke.resolve(ret)
    }

    /** Creates a fresh key, then seals `data` once the user passes the prompt. */
    @Command
    fun seal(invoke: Invoke) {
        val args = invoke.parseArgs(SealArgs::class.java)
        val key = try {
            deleteKey()
            createKey()
        } catch (e: Exception) {
            invoke.reject("The biometric key could not be created: ${e.message}", "unavailable")
            return
        }
        val cipher = { Cipher.getInstance(TRANSFORMATION).apply { init(Cipher.ENCRYPT_MODE, key) } }
        authenticate(invoke, args, cipher) { unlocked ->
            val data = Base64.decode(args.data, Base64.NO_WRAP)
            try {
                val ciphertext = unlocked.doFinal(data)
                val ret = JSObject()
                ret.put("iv", Base64.encodeToString(unlocked.iv, Base64.NO_WRAP))
                ret.put("ciphertext", Base64.encodeToString(ciphertext, Base64.NO_WRAP))
                ret
            } finally {
                data.fill(0)
            }
        }
    }

    /** Unseals once the user passes the prompt. */
    @Command
    fun open(invoke: Invoke) {
        val args = invoke.parseArgs(OpenArgs::class.java)
        if (!keyStore().containsAlias(KEY_ALIAS)) {
            invoke.reject("There is no biometric key.", "invalidated")
            return
        }
        val cipher = {
            val key = keyStore().getKey(KEY_ALIAS, null) as SecretKey?
                ?: throw KeyPermanentlyInvalidatedException("There is no biometric key.")
            Cipher.getInstance(TRANSFORMATION).apply {
                init(
                    Cipher.DECRYPT_MODE,
                    key,
                    GCMParameterSpec(TAG_BITS, Base64.decode(args.iv, Base64.NO_WRAP))
                )
            }
        }
        authenticate(invoke, args, cipher) { unlocked ->
            val data = unlocked.doFinal(Base64.decode(args.ciphertext, Base64.NO_WRAP))
            try {
                val ret = JSObject()
                ret.put("data", Base64.encodeToString(data, Base64.NO_WRAP))
                ret
            } finally {
                data.fill(0)
            }
        }
    }

    @Command
    fun forget(invoke: Invoke) {
        try {
            deleteKey()
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("The biometric key could not be deleted: ${e.message}", "failed")
        }
    }

    private fun keyStore(): KeyStore = KeyStore.getInstance(KEYSTORE).apply { load(null) }

    private fun deleteKey() {
        val store = keyStore()
        if (store.containsAlias(KEY_ALIAS)) store.deleteEntry(KEY_ALIAS)
    }

    private fun createKey(): SecretKey {
        val spec = KeyGenParameterSpec.Builder(
            KEY_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setUserAuthenticationRequired(true)
            .setInvalidatedByBiometricEnrollment(true)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            // Every use needs its own strong biometric check.
            spec.setUserAuthenticationParameters(0, KeyProperties.AUTH_BIOMETRIC_STRONG)
        } else {
            @Suppress("DEPRECATION")
            spec.setUserAuthenticationValidityDurationSeconds(-1)
        }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE).run {
            init(spec.build())
            generateKey()
        }
    }

    /** A prompt asked for, until it settles. */
    private class Request(
        val invoke: Invoke,
        val info: BiometricPrompt.PromptInfo,
        /** A fresh cipher for each showing of the prompt. */
        val cipher: () -> Cipher,
        val finish: (Cipher) -> JSObject
    )

    // Touched on the UI thread only.
    /** The one prompt asked for and not yet settled. */
    private var current: Request? = null
    /** The prompt on screen for `current`, if it is. */
    private var showing: BiometricPrompt? = null
    /** Waiting for the window's focus to show `current`. */
    private var awaitingFocus = false

    /**
     * Shows the system prompt with a cipher from `cipher` as its
     * CryptoObject; once the user passes it, the hardware has unlocked the
     * cipher, and `finish` uses it. Resolves or rejects `invoke` either way.
     *
     * The prompt only shows while the app is in the foreground: asked for
     * in the background (Arsu locks the vault as the screen goes off, and
     * its lock screen asks for the fingerprint at once), it shows when the
     * app comes back; and one on screen as the app leaves is withdrawn,
     * and shown again on return (see [onStop]).
     */
    private fun authenticate(
        invoke: Invoke,
        args: PromptArgs,
        cipher: () -> Cipher,
        finish: (Cipher) -> JSObject
    ) {
        val host = activity as? FragmentActivity
        if (host == null) {
            invoke.reject("The app's activity cannot show a biometric prompt.", "unavailable")
            return
        }
        val info = BiometricPrompt.PromptInfo.Builder()
            .setTitle(args.title)
            .setSubtitle(args.subtitle)
            .setNegativeButtonText(args.cancel)
            .setAllowedAuthenticators(BIOMETRIC_STRONG)
            .setConfirmationRequired(false)
            .build()
        activity.runOnUiThread {
            if (current != null) {
                // androidx would ignore a second prompt, leaving it unanswered.
                invoke.reject("A biometric prompt is already open.", "failed")
                return@runOnUiThread
            }
            val request = Request(invoke, info, cipher, finish)
            current = request
            showWhenReady(host)
        }
    }

    override fun onResume(activity: AppCompatActivity) {
        // Once the resume has finished, the fragments' state included.
        activity.window.decorView.post { showWhenReady(activity) }
    }

    /**
     * Withdraws the prompt on screen as the app leaves the foreground (the
     * screen going off, another app), to show it again on return. The
     * system cancels it then anyway, but may not say so (the session is no
     * longer current, and its error is dropped), and androidx, never told,
     * would ignore every prompt after it. Cancelling it here resets androidx
     * whatever the system does; any answer from it after this is ignored.
     */
    override fun onStop(activity: AppCompatActivity) {
        val prompt = showing ?: return
        showing = null
        prompt.cancelAuthentication()
    }

    override fun onDestroy(activity: AppCompatActivity) {
        showing = null
        val request = current ?: return
        settle(request) { it.reject("The app closed.", "cancelled") }
    }

    /**
     * Shows the current prompt, unless it is on screen already, once the
     * app is in the foreground and its window has the focus: the window
     * only gets it once the lock screen has gone, and a prompt asked for
     * before then may never show.
     */
    private fun showWhenReady(host: FragmentActivity) {
        val request = current ?: return
        if (showing != null || !inForeground(host)) return
        if (host.hasWindowFocus()) {
            show(host, request)
            return
        }
        if (awaitingFocus) return
        awaitingFocus = true
        val view = host.window.decorView
        view.viewTreeObserver.addOnWindowFocusChangeListener(object :
            ViewTreeObserver.OnWindowFocusChangeListener {
            override fun onWindowFocusChanged(hasFocus: Boolean) {
                if (!hasFocus) return
                view.viewTreeObserver.removeOnWindowFocusChangeListener(this)
                awaitingFocus = false
                view.post { showWhenReady(host) }
            }
        })
    }

    private fun inForeground(host: FragmentActivity): Boolean =
        host.lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED) &&
            !host.supportFragmentManager.isStateSaved

    /** Ends `request`, if it is still the current one, with `answer`. */
    private fun settle(request: Request, answer: (Invoke) -> Unit) {
        if (current !== request) return
        current = null
        showing = null
        answer(request.invoke)
    }

    private fun show(host: FragmentActivity, request: Request) {
        val cipher = try {
            request.cipher()
        } catch (e: KeyPermanentlyInvalidatedException) {
            deleteKey()
            settle(request) {
                it.reject(e.message ?: "The phone's fingerprints or faces changed.", "invalidated")
            }
            return
        } catch (e: Exception) {
            settle(request) { it.reject("The biometric key could not be used: ${e.message}", "unavailable") }
            return
        }
        lateinit var prompt: BiometricPrompt
        // Only the prompt on screen answers: not one withdrawn by `onStop`.
        fun answers() = showing === prompt
        val callback = object : BiometricPrompt.AuthenticationCallback() {
            override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                if (!answers()) return
                val unlocked = result.cryptoObject?.cipher
                settle(request) {
                    if (unlocked == null) {
                        it.reject("The prompt returned no cipher.", "failed")
                        return@settle
                    }
                    try {
                        it.resolve(request.finish(unlocked))
                    } catch (e: Exception) {
                        it.reject("The data could not be sealed or unsealed: ${e.message}", "failed")
                    }
                }
            }

            override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                if (!answers()) return
                val code = when (errorCode) {
                    BiometricPrompt.ERROR_USER_CANCELED,
                    BiometricPrompt.ERROR_NEGATIVE_BUTTON,
                    BiometricPrompt.ERROR_CANCELED -> "cancelled"
                    BiometricPrompt.ERROR_LOCKOUT,
                    BiometricPrompt.ERROR_LOCKOUT_PERMANENT -> "lockout"
                    BiometricPrompt.ERROR_NO_BIOMETRICS,
                    BiometricPrompt.ERROR_HW_NOT_PRESENT,
                    BiometricPrompt.ERROR_HW_UNAVAILABLE -> "unavailable"
                    else -> "failed"
                }
                settle(request) { it.reject(errString.toString(), code) }
            }
        }
        prompt = BiometricPrompt(host, ContextCompat.getMainExecutor(activity), callback)
        showing = prompt
        prompt.authenticate(request.info, BiometricPrompt.CryptoObject(cipher))
    }
}
