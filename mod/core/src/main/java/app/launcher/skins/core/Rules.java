package app.launcher.skins.core;

/**
 * What a texture may be (D-065). skins.lapi.sh checks the same before it stores one; the
 * mod checks again before it gives a texture to the game.
 */
public final class Rules {
    public static final int MAX_FRAMES = 32;
    public static final int MIN_FRAME_MS = 50;
    public static final int MAX_FRAME_MS = 10_000;
    public static final int MAX_SKIN_BYTES = 2 * 1024 * 1024;
    public static final int MAX_CAPE_BYTES = 1024 * 1024;
    public static final long MAX_PIXELS = 16_777_216L;

    public enum Kind {
        SKIN,
        CAPE;

        public int maxBytes() {
            return this == SKIN ? MAX_SKIN_BYTES : MAX_CAPE_BYTES;
        }
    }

    private Rules() {}

    /** Whether a frame may be this wide: 64 × 1, 2, 4 or 8. */
    public static boolean width(int width) {
        return width == 64 || width == 128 || width == 256 || width == 512;
    }

    /**
     * The height of one frame of a texture this wide, or -1 if the width is not allowed.
     * A skin frame is square, a cape frame half as high as it is wide.
     */
    public static int frameHeight(Kind kind, int width) {
        if (!width(width)) {
            return -1;
        }
        return kind == Kind.SKIN ? width : width / 2;
    }

    /**
     * How many frames an image of this size holds (a vertical strip), or -1 if the size is
     * not allowed. A skin half as high as it is wide (the format before 1.8: 64×32, or HD up
     * to 512×256) is allowed as one frame only.
     */
    public static int frames(Kind kind, int width, int height) {
        if (width <= 0 || height <= 0 || (long) width * height > MAX_PIXELS) {
            return -1;
        }
        if (legacy(kind, width, height)) {
            return 1;
        }
        int frame = frameHeight(kind, width);
        if (frame < 0 || height % frame != 0) {
            return -1;
        }
        int frames = height / frame;
        return frames >= 1 && frames <= MAX_FRAMES ? frames : -1;
    }

    /** Whether this is a skin in the format before 1.8, at any allowed width. */
    public static boolean legacy(Kind kind, int width, int height) {
        return kind == Kind.SKIN && width(width) && (long) height * 2 == width;
    }

    /** Whether frame durations fit a texture of this many frames: one for all, or one each. */
    public static boolean frameMs(int[] ms, int frames) {
        if (ms == null || (ms.length != 1 && ms.length != frames)) {
            return false;
        }
        for (int each : ms) {
            if (each < MIN_FRAME_MS || each > MAX_FRAME_MS) {
                return false;
            }
        }
        return true;
    }
}
