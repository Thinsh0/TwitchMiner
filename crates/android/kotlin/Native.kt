package dev.thinsh0.twitchminer

/**
 * Thin wrapper over libtwitchminer.so (crate `tm-android`).
 * All payloads are the JSON documented in docs/API.md.
 */
object Native {
    init {
        System.loadLibrary("twitchminer")
    }

    /** Starts the engine. [apiBind] e.g. "0.0.0.0:7878" to allow remote control, or null. */
    @JvmStatic external fun start(dataDir: String, apiBind: String?): Long

    /** Executes a command, e.g. """{"cmd":"snapshot"}""". Returns {"ok":…,"data"|"error":…}. */
    @JvmStatic external fun execute(handle: Long, commandJson: String): String

    /** Compact view for the ongoing notification / OneUI Now Bar. */
    @JvmStatic external fun summary(handle: Long): String

    @JvmStatic external fun apiToken(handle: Long): String?

    /** Stops the engine; the handle must not be used afterwards. */
    @JvmStatic external fun stop(handle: Long)
}
