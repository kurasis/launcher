package app.launcher.skins.core;

/** The size a PNG declares, read from its header before anything decodes it. */
public final class Png {
    private static final byte[] SIGNATURE = {(byte) 0x89, 'P', 'N', 'G', '\r', '\n', 0x1A, '\n'};

    private Png() {}

    /** Width and height from the IHDR chunk, or null if this is not a PNG. */
    public static int[] size(byte[] png) {
        if (png == null || png.length < 33) {
            return null;
        }
        for (int i = 0; i < SIGNATURE.length; i++) {
            if (png[i] != SIGNATURE[i]) {
                return null;
            }
        }
        if (int32(png, 8) != 13
                || png[12] != 'I'
                || png[13] != 'H'
                || png[14] != 'D'
                || png[15] != 'R') {
            return null;
        }
        int width = int32(png, 16);
        int height = int32(png, 20);
        return width > 0 && height > 0 ? new int[] {width, height} : null;
    }

    private static int int32(byte[] b, int at) {
        return (b[at] & 0xFF) << 24 | (b[at + 1] & 0xFF) << 16 | (b[at + 2] & 0xFF) << 8 | (b[at + 3] & 0xFF);
    }
}
