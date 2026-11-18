package dev.local.hifi

import android.app.Application
import android.os.Build
import java.io.File
import java.io.PrintWriter
import java.io.StringWriter

/** Keeps a note of what brought the app down, for the user to read and pass on if they choose
 *  (see src-tauri/src/crash.rs: the app itself sends it nowhere). Set up before anything else
 *  of the app runs, so a crash right at launch is noted too. */
class App : Application() {
  override fun onCreate() {
    super.onCreate()
    // The folder Rust knows as the app's data folder.
    val dir = dataDir
    val system = "Android ${Build.VERSION.RELEASE} (SDK ${Build.VERSION.SDK_INT}), ${Build.MANUFACTURER} ${Build.MODEL}"
    // For reports the Rust side writes: it can't ask Android for these itself.
    runCatching { File(dir, "system.txt").writeText(system) }
    val before = Thread.getDefaultUncaughtExceptionHandler()
    Thread.setDefaultUncaughtExceptionHandler { thread, error ->
      runCatching {
        val trace = StringWriter().also { error.printStackTrace(PrintWriter(it)) }.toString().take(6000)
        val version = runCatching { packageManager.getPackageInfo(packageName, 0).versionName }.getOrNull() ?: "?"
        val file = File(dir, "crash.txt")
        // A crash loop mustn't fill the storage.
        if (file.length() > 64 * 1024) file.delete()
        // Paths (a file that couldn't be opened) are taken out when the report is shown.
        file.appendText("BitSpring $version — error report\nSystem: $system\nThread: ${thread.name}\nError (Android):\n$trace\n----\n")
      }
      before?.uncaughtException(thread, error)
    }
  }
}
