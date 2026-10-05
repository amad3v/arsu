import java.io.File
import org.apache.tools.ant.taskdefs.condition.Os
import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.logging.LogLevel
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.TaskAction
import javax.inject.Inject
import org.gradle.process.ExecOperations

abstract class BuildTask : DefaultTask() {
    @get:Inject
    abstract val execOperations: ExecOperations

    @Input
    var rootDirRel: String? = null
    @Input
    var projectDir: String? = null
    @Input
    var target: String? = null
    @Input
    var release: Boolean? = null

    @TaskAction
    fun assemble() {
        val executable = """pnpm""";
        try {
            runTauriCli(executable)
        } catch (e: Exception) {
            if (Os.isFamily(Os.FAMILY_WINDOWS)) {
                // Try different Windows-specific extensions
                val fallbacks = listOf(
                    "$executable.exe",
                    "$executable.cmd",
                    "$executable.bat",
                )

                var lastException: Exception = e
                for (fallback in fallbacks) {
                    try {
                        runTauriCli(fallback)
                        return
                    } catch (fallbackException: Exception) {
                        lastException = fallbackException
                    }
                }
                throw lastException
            } else {
                throw e;
            }
        }
    }

    fun runTauriCli(executable: String) {
        val rootDirRel = rootDirRel ?: throw GradleException("rootDirRel cannot be null")
        val target = target ?: throw GradleException("target cannot be null")
        val release = release ?: throw GradleException("release cannot be null")
        val args = listOf("tauri", "android", "android-studio-script");

        execOperations.exec {
            workingDir(File(projectDir, rootDirRel))
            executable(executable)
            args(args)
            if (logger.isEnabled(LogLevel.DEBUG)) {
                args("-vv")
            } else if (logger.isEnabled(LogLevel.INFO)) {
                args("-v")
            }
            if (release) {
                args("--release")
            }
            args(listOf("--target", target))
            environment("CARGO_ENCODED_RUSTFLAGS", rustFlags().joinToString(FLAG_SEPARATOR))
        }.assertNormalExitValue()
    }

    /**
     * The caller's rustc flags, plus a `--remap-path-prefix` for the Cargo
     * and rustup homes: their absolute paths would otherwise end up in the
     * library (in panic locations), and the APK would differ from one
     * machine to the next, which F-Droid's reproducible build forbids.
     * Passed encoded, so that paths with spaces survive.
     */
    private fun rustFlags(): List<String> {
        val home = System.getProperty("user.home")
        val cargoHome = System.getenv("CARGO_HOME") ?: "$home/.cargo"
        val rustupHome = System.getenv("RUSTUP_HOME") ?: "$home/.rustup"
        val callerFlags = System.getenv("CARGO_ENCODED_RUSTFLAGS")
            ?.takeIf { it.isNotEmpty() }
            ?.split(FLAG_SEPARATOR)
            ?: System.getenv("RUSTFLAGS").orEmpty().split(Regex("\\s+")).filter { it.isNotEmpty() }
        return callerFlags + listOf(
            "--remap-path-prefix=$cargoHome=/cargo",
            "--remap-path-prefix=$rustupHome=/rustup",
        )
    }

    private companion object {
        const val FLAG_SEPARATOR = "\u001f"
    }
}