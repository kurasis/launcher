package app.launcher.skins.core;

/**
 * Pixel work on a skin before the game gets it, on plain ARGB arrays so every game version
 * shares it: the format before 1.8 (64×32 × k, one arm and one leg) becomes the current one, and
 * the inner layer is made opaque, as the game does for its own skins. HD skins scale every
 * rectangle by their width / 64.
 */
public final class SkinImage {
    private SkinImage() {}

    /**
     * A skin in the format before 1.8 (64×32 × k) as a square one: the left arm and leg mirror
     * the right ones, the second layer is empty.
     */
    public static int[] upgradeLegacy(int[] argb, int width) {
        if (width < 64 || width % 64 != 0 || argb.length != width * (width / 2)) {
            throw new IllegalArgumentException("not a skin in the format before 1.8");
        }
        int[] out = new int[width * width];
        System.arraycopy(argb, 0, out, 0, argb.length);
        int k = width / 64;
        mirrorLimb(out, width, k, 0, 16, 16, 48); // right leg → left leg
        mirrorLimb(out, width, k, 40, 16, 32, 48); // right arm → left arm
        return out;
    }

    /**
     * Copies a 4×12×4 limb at (su, sv) to (du, dv), in 64×64 units scaled by k, mirrored as
     * the other side of the body: each face flipped, the outer and inner sides swapped.
     */
    private static void mirrorLimb(int[] px, int width, int k, int su, int sv, int du, int dv) {
        flip(px, width, k, su + 4, sv, du + 4, dv, 4, 4); // top
        flip(px, width, k, su + 8, sv, du + 8, dv, 4, 4); // bottom
        flip(px, width, k, su + 8, sv + 4, du, dv + 4, 4, 12); // inner side → outer side
        flip(px, width, k, su + 4, sv + 4, du + 4, dv + 4, 4, 12); // front
        flip(px, width, k, su, sv + 4, du + 8, dv + 4, 4, 12); // outer side → inner side
        flip(px, width, k, su + 12, sv + 4, du + 12, dv + 4, 4, 12); // back
    }

    private static void flip(int[] px, int width, int k, int su, int sv, int du, int dv, int w, int h) {
        int pw = w * k;
        for (int y = 0; y < h * k; y++) {
            for (int x = 0; x < pw; x++) {
                px[(dv * k + y) * width + du * k + x] = px[(sv * k + y) * width + su * k + pw - 1 - x];
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
