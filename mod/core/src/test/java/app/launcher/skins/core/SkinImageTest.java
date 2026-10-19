package app.launcher.skins.core;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

import org.junit.jupiter.api.Test;

class SkinImageTest {
    @Test
    void legacySkinsGetMirroredLimbs() {
        int[] legacy = new int[64 * 32];
        for (int i = 0; i < legacy.length; i++) {
            legacy[i] = 0xFF000000 | i;
        }
        int[] skin = SkinImage.upgradeLegacy(legacy);
        assertEquals(64 * 64, skin.length);
        // Right leg front (4..8, 20) → left leg front (20..24, 52), flipped.
        assertEquals(legacy[20 * 64 + 7], skin[52 * 64 + 20]);
        assertEquals(legacy[20 * 64 + 4], skin[52 * 64 + 23]);
        // Right leg outer side (0, 20) → left leg inner side (24, 52).
        assertEquals(legacy[20 * 64 + 3], skin[52 * 64 + 24]);
        // Right arm top (44, 16) → left arm top (36, 48).
        assertEquals(legacy[16 * 64 + 47], skin[48 * 64 + 36]);
        // The second layer of the legs stays empty.
        assertEquals(0, skin[52 * 64 + 4]);
        assertThrows(IllegalArgumentException.class, () -> SkinImage.upgradeLegacy(new int[64 * 64]));
    }

    @Test
    void innerLayerIsOpaqueAtAnySize() {
        int[] hd = new int[128 * 128];
        SkinImage.opaqueInnerLayer(hd, 128);
        assertEquals(0xFF000000, hd[0]);
        assertEquals(0xFF000000, hd[(16 * 2) * 128 + 127]);
        assertEquals(0, hd[(16 * 2 - 1) * 128 + 127], "the hat stays as drawn");
        assertEquals(0, hd[(48 * 2) * 128 + 0], "the left leg's second layer stays as drawn");
    }
}
