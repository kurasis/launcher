package app.launcher.skins.game;

/**
 * The coremod's hook in {@code SoundManager.reloadSoundSystem} (see
 * {@code SoundTransformer}): waits for the game's "Sound Library Loader" thread, which
 * starts a sound system, so the reload then stops that system before it starts another.
 * The loader gives up on a sound system after 30 seconds, so the wait is bounded.
 */
public final class SoundStart {
    private static final String LOADER = "Sound Library Loader";
    private static final long WAIT_MS = 35_000;

    private SoundStart() {}

    public static void await() {
        for (Thread thread : Thread.getAllStackTraces().keySet()) {
            if (thread != Thread.currentThread() && LOADER.equals(thread.getName())) {
                try {
                    thread.join(WAIT_MS);
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                    return;
                }
            }
        }
    }
}
