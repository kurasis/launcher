package app.launcher.skins.core;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertNull;

import java.awt.image.BufferedImage;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.util.Arrays;
import javax.imageio.ImageIO;
import org.junit.jupiter.api.Test;

class PngTest {
    static byte[] png(int width, int height) {
        try {
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            ImageIO.write(new BufferedImage(width, height, BufferedImage.TYPE_INT_ARGB), "png", out);
            return out.toByteArray();
        } catch (IOException e) {
            throw new IllegalStateException(e);
        }
    }

    @Test
    void readsTheHeader() {
        assertArrayEquals(new int[] {64, 32}, Png.size(png(64, 32)));
        assertArrayEquals(new int[] {512, 4096}, Png.size(png(512, 4096)));
    }

    @Test
    void refusesJunk() {
        byte[] good = png(64, 64);
        assertNull(Png.size(null));
        assertNull(Png.size(new byte[0]));
        assertNull(Png.size(Arrays.copyOf(good, 20)));
        assertNull(Png.size("GIF89a not a png at all, just text".getBytes()));
        byte[] wrongChunk = good.clone();
        wrongChunk[12] = 'X';
        assertNull(Png.size(wrongChunk));
        byte[] negative = good.clone();
        negative[16] = (byte) 0x80;
        assertNull(Png.size(negative));
    }
}
