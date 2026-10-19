package app.launcher.skins.core;

/**
 * Pixel work on a skin before the game gets it, on plain ARGB arrays so every game version
 * shares it: the format before 1.8 (64×32, one arm and one leg) becomes the current one, and
 * the inner layer is made opaque, as the game does for its own skins. HD skins scale every
 * rectangle by their width / 64.
 */
public final class SkinImage {
    private SkinImage() {}

    /** A 64×32 skin as 64×64: the left arm and leg mirror the right ones, the second layer is empty. */
    public static int[] upgradeLegacy(int[] argb) {
        if (argb.length != 64 * 32) {
            throw new IllegalArgumentException("not a 64x32 skin");
        }
        int[] out = new int[64 * 64];
        System.arraycopy(argb, 0, out, 0, argb.length);
        mirrorLimb(out, 0, 16, 16, 48); // right leg → left leg
        mirrorLimb(out, 40, 16, 32, 48); // right arm → left arm
        return out;
    }

    /**
     * Copies a 4×12×4 limb at (su, sv) to (du, dv), mirrored as the other side of the body:
     * each face flipped, the outer and inner sides swapped.
     */
    private static void mirrorLimb(int[] px, int su, int sv, int du, int dv) {
        flip(px, su + 4, sv, du + 4, dv, 4, 4); // top
        flip(px, su + 8, sv, du + 8, dv, 4, 4); // bottom
        flip(px, su + 8, sv + 4, du, dv + 4, 4, 12); // inner side → outer side
        flip(px, su + 4, sv + 4, du + 4, dv + 4, 4, 12); // front
        flip(px, su, sv + 4, du + 8, dv + 4, 4, 12); // outer side → inner side
        flip(px, su + 12, sv + 4, du + 12, dv + 4, 4, 12); // back
    }

    private static void flip(int[] px, int su, int sv, int du, int dv, int w, int h) {
        for (int y = 0; y < h; y++) {
            for (int x = 0; x < w; x++) {
                px[(dv + y) * 64 + du + x] = px[(sv + y) * 64 + su + w - 1 - x];
            }
        }
    }

    /** Makes the inner layer of a square skin (any HD size) opaque, in place. */
    public static void opaqueInnerLayer(int[] argb, int width) {
        int k = width / 64;
        opaque(argb, width, 0, 0, 32 * k, 16 * k);
        opaque(argb, width, 0, 16 * k, 64 * k, 32 * k);
        opaque(argb, width, 16 * k, 48 * k, 48 * k, 64 * k);
    }

    private static void opaque(int[] px, int width, int x0, int y0, int x1, int y1) {
        for (int y = y0; y < y1; y++) {
            for (int x = x0; x < x1; x++) {
                px[y * width + x] |= 0xFF000000;
            }
        }
    }
}
