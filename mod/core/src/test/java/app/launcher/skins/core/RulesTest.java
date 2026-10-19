package app.launcher.skins.core;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import app.launcher.skins.core.Rules.Kind;
import java.util.UUID;
import org.junit.jupiter.api.Test;

class RulesTest {
    @Test
    void skinSizes() {
        assertEquals(1, Rules.frames(Kind.SKIN, 64, 64));
        assertEquals(1, Rules.frames(Kind.SKIN, 64, 32));
        assertEquals(1, Rules.frames(Kind.SKIN, 128, 128));
        assertEquals(1, Rules.frames(Kind.SKIN, 512, 512));
        assertEquals(8, Rules.frames(Kind.SKIN, 64, 512));
        assertEquals(32, Rules.frames(Kind.SKIN, 512, 512 * 32));
        for (int[] bad : new int[][] {{65, 65}, {64, 48}, {1024, 1024}, {64, 96}, {64, 64 * 33}, {128, 64},
            {0, 64}, {-64, 64}, {64, 0}, {512, Integer.MAX_VALUE}}) {
            assertEquals(-1, Rules.frames(Kind.SKIN, bad[0], bad[1]), bad[0] + "x" + bad[1]);
        }
    }

    @Test
    void capeSizes() {
        assertEquals(1, Rules.frames(Kind.CAPE, 64, 32));
        assertEquals(1, Rules.frames(Kind.CAPE, 512, 256));
        assertEquals(4, Rules.frames(Kind.CAPE, 128, 256));
        assertEquals(-1, Rules.frames(Kind.CAPE, 64, 64 * 17));
        assertEquals(-1, Rules.frames(Kind.CAPE, 46, 22));
        assertEquals(-1, Rules.frames(Kind.CAPE, 64, 48));
    }

    @Test
    void frameDurations() {
        assertTrue(Rules.frameMs(new int[] {100}, 4));
        assertTrue(Rules.frameMs(new int[] {50, 10_000, 200}, 3));
        assertFalse(Rules.frameMs(new int[] {49}, 2));
        assertFalse(Rules.frameMs(new int[] {10_001}, 2));
        assertFalse(Rules.frameMs(new int[] {100, 100}, 3));
        assertFalse(Rules.frameMs(new int[0], 3));
        assertFalse(Rules.frameMs(null, 3));
    }

    @Test
    void framesFollowTheClock() {
        int[] ms = {100, 300};
        assertEquals(0, Frames.at(2, ms, 0));
        assertEquals(0, Frames.at(2, ms, 99));
        assertEquals(1, Frames.at(2, ms, 100));
        assertEquals(1, Frames.at(2, ms, 399));
        assertEquals(0, Frames.at(2, ms, 400));
        assertEquals(1, Frames.at(2, ms, -1));
        assertEquals(2, Frames.at(4, new int[] {50}, 125));
        assertEquals(0, Frames.at(1, new int[0], 12345));
    }

    @Test
    void nicks() {
        for (String ok : new String[] {"abc", "Player_01", "abcdefghijklmnop"}) {
            assertTrue(Nicks.valid(ok), ok);
        }
        for (String bad : new String[] {null, "", "ab", "abcdefghijklmnopq", "../x", "a b", "Jürgen", "аbc",
            "abc%00", "abc\n", "abc/def"}) {
            assertFalse(Nicks.valid(bad), String.valueOf(bad));
        }
        assertEquals("player_01", Nicks.key("Player_01"));
    }

    @Test
    void whoseSkin() {
        UUID offline = UUID.nameUUIDFromBytes("OfflinePlayer:Steve".getBytes());
        UUID microsoft = UUID.randomUUID();
        assertTrue(Nicks.ours("Steve", offline, false));
        assertFalse(Nicks.ours("Steve", offline, true), "textures of its own win");
        assertFalse(Nicks.ours("Steve", microsoft, false), "never a Microsoft account");
        assertFalse(Nicks.ours("../x", offline, false));
        assertFalse(Nicks.ours("Steve", null, false));
    }
}
