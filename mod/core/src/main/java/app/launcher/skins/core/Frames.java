package app.launcher.skins.core;

/** Which frame of an animated texture shows at a moment. */
public final class Frames {
    private Frames() {}

    /**
     * The frame showing at {@code nowMs} (any clock shared by everyone, so all players see
     * the same frame). Durations are one for every frame or one per frame.
     */
    public static int at(int frames, int[] ms, long nowMs) {
        if (frames <= 1 || ms == null || ms.length == 0) {
            return 0;
        }
        long total = 0;
        for (int i = 0; i < frames; i++) {
            total += duration(ms, i);
        }
        long t = Math.floorMod(nowMs, total);
        for (int i = 0; i < frames; i++) {
            t -= duration(ms, i);
            if (t < 0) {
                return i;
            }
        }
        return frames - 1;
    }

    private static int duration(int[] ms, int frame) {
        return ms.length == 1 ? ms[0] : ms[frame];
    }
}
