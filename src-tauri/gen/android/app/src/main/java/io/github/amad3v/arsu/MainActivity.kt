package io.github.amad3v.arsu

import android.os.Bundle
import android.view.View
import android.view.WindowManager
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  private var webView: WebView? = null

  /**
   * Back (the button or the gesture) asks the page to close its overlay on
   * top: a page, a sheet or a dialog. When none is open, the next handler
   * follows (Tauri's, then Android's), which leaves the app.
   *
   * Not the WebView history, which Tauri's own handler goes back through:
   * the WebView skips history entries a page adds by script, so Back could
   * leave the app from a page of it still open.
   */
  private val backCallback = object : OnBackPressedCallback(true) {
    override fun handleOnBackPressed() {
      val page = webView ?: return passBack()
      page.evaluateJavascript("window.__arsuBack ? window.__arsuBack() : false") { handled ->
        if (handled != "true") passBack()
      }
    }

    private fun passBack() {
      isEnabled = false
      onBackPressedDispatcher.onBackPressed()
      isEnabled = true
    }
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    // No screenshots or screen recordings of the codes, and a blank
    // thumbnail in the recent apps list.
    window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    keepContentClearOfSystemBars()
  }

  /**
   * The window draws behind the status and navigation bars (Android 15
   * enforces it), and the WebView knows nothing of them: pad the content
   * by the bars, the display cutout and the on-screen keyboard instead,
   * so the interface is laid out in the space left between them.
   */
  private fun keepContentClearOfSystemBars() {
    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val bars = insets.getInsets(
        WindowInsetsCompat.Type.systemBars() or
          WindowInsetsCompat.Type.displayCutout() or
          WindowInsetsCompat.Type.ime()
      )
      view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      WindowInsetsCompat.CONSUMED
    }
  }

  override fun onWebViewCreate(webView: WebView) {
    this.webView = webView
    runOnUiThread { putBackCallbackOnTop() }
  }

  override fun onResume() {
    super.onResume()
    putBackCallbackOnTop()
  }

  /**
   * The handler added last runs first: keep this one above Tauri's, which
   * its app plugin adds as the app starts.
   */
  private fun putBackCallbackOnTop() {
    backCallback.remove()
    onBackPressedDispatcher.addCallback(this, backCallback)
  }
}
