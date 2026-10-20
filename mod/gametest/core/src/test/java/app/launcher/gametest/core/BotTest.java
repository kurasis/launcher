package app.launcher.gametest.core;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Properties;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class BotTest {
    /** A game that does what it is told at once. */
    static class FakeGame implements Game {
        String screen = "net.minecraft.client.gui.screens.TitleScreen";
        boolean loading = true;
        boolean world;
        int ticksToWorld = -1;
        boolean quit;
        String skin;
        String[] skinFrames = {};
        String cape;
        Boolean slim;
        long tick;
        final List<String> commands = new ArrayList<>();
        final List<String> shots = new ArrayList<>();

        void step() {
            tick++;
            if (ticksToWorld > 0 && --ticksToWorld == 0) {
                world = true;
                screen = null;
            }
        }

        @Override
        public String screen() {
            return screen;
        }

        @Override
        public boolean loading() {
            return loading;
        }

        @Override
        public boolean onTitle() {
            return screen != null && screen.endsWith("TitleScreen");
        }

        @Override
        public boolean worldOpen() {
            return world;
        }

        @Override
        public void createWorld(String name, long seed) {
            screen = "net.minecraft.client.gui.screens.LevelLoadingScreen";
            ticksToWorld = 40;
        }

        @Override
        public void unattended() {
        }

        @Override
        public int fps() {
            return 120;
        }

        @Override
        public int chunks() {
            return 300;
        }

        @Override
        public int entities() {
            return 10;
        }

        @Override
        public double tickMs() {
            return 12.5;
        }

        @Override
        public void keys(boolean forward, boolean left, boolean sprint, boolean jump) {
        }

        @Override
        public float yaw() {
            return 0;
        }

        @Override
        public void look(float yaw, float pitch) {
        }

        @Override
        public void command(String command) {
            commands.add(command);
        }

        @Override
        public void camera(int view) {
        }

        @Override
        public void flying(boolean flying) {
        }

        @Override
        public boolean dead() {
            return false;
        }

        @Override
        public void respawn() {
        }

        @Override
        public void screenshot(String fileName) {
            shots.add(fileName);
        }

        @Override
        public void openInventory() {
            screen = "net.minecraft.client.gui.screens.inventory.InventoryScreen";
        }

        @Override
        public void closeScreen() {
            screen = null;
        }

        @Override
        public String skinTexture() {
            if (skinFrames.length > 0) {
                return skinFrames[(int) (tick / 10 % skinFrames.length)];
            }
            return skin;
        }

        @Override
        public String capeTexture() {
            return cape;
        }

        @Override
        public Boolean slim() {
            return slim;
        }

        @Override
        public void quit() {
            quit = true;
        }
    }

    private static Config config(File out, String... pairs) {
        Properties p = new Properties();
        p.setProperty("launchergametest.out", out.getPath());
        p.setProperty("launchergametest.minutes", "1");
        p.setProperty("launchergametest.phaseSeconds", "10");
        for (int i = 0; i < pairs.length; i += 2) {
            p.setProperty("launchergametest." + pairs[i], pairs[i + 1]);
        }
        return Config.from(p);
    }

    /** Runs the bot at 20 ticks a second until it closes the game or 20 minutes pass. */
    private static Report run(Bot bot, FakeGame game, long[] now) {
        for (int i = 0; i < 20 * 60 * 20 && !game.quit; i++) {
            if (i == 100) {
                game.loading = false;
            }
            bot.tick(game);
            game.step();
            now[0] += 50_000_000L;
        }
        return bot.report();
    }

    @Test
    void noBotWithoutAnOutputFolder() {
        assertNull(Config.from(new Properties()));
    }

    @Test
    void playsTheWholeTimeAndChecksTheSkin(@TempDir Path dir) throws Exception {
        long[] now = {0};
        Bot bot = new Bot(config(dir.toFile(), "skin", "slim", "skinFrames", "4", "capeFrames", "1"), () -> now[0]);
        FakeGame game = new FakeGame();
        game.skinFrames = new String[] {"launcherskins:skin/a/0", "launcherskins:skin/a/1",
            "launcherskins:skin/a/2", "launcherskins:skin/a/3"};
        game.cape = "launcherskins:cape/b/0";
        game.slim = true;
        Report report = run(bot, game, now);

        assertTrue(game.quit);
        assertTrue(report.finished);
        assertTrue(report.passed(), report.toJson());
        assertEquals(6, report.phases.size());
        assertTrue(report.playedMs >= 59_000);
        assertEquals(4, report.skinFramesSeen);
        assertTrue(game.commands.contains("gamemode creative"));
        assertTrue(game.commands.stream().anyMatch(c -> c.startsWith("spreadplayers ")));
        assertTrue(game.commands.stream().anyMatch(c -> c.startsWith("summon minecraft:zombie")));
        assertTrue(game.shots.contains("gametest-01-skin-front.png"), game.shots.toString());
        String json = new String(Files.readAllBytes(dir.resolve("report.json")), StandardCharsets.UTF_8);
        assertTrue(json.contains("\"passed\":true"), json);
        assertTrue(json.contains("\"name\":\"far\""));
    }

    @Test
    void aWarningScreenBeforeTheTitleFailsTheRun(@TempDir Path dir) {
        long[] now = {0};
        Bot bot = new Bot(config(dir.toFile()), () -> now[0]);
        FakeGame game = new FakeGame();
        game.screen = "net.minecraftforge.client.gui.LoadingErrorScreen";
        Report report = run(bot, game, now);

        assertTrue(report.finished);
        assertFalse(report.passed());
        assertEquals("title-screen", report.checks.get(0).name);
        assertFalse(report.checks.get(0).ok);
        assertTrue(game.shots.contains("gametest-01-stuck.png"));
        assertNotNull(report.problems.get(0));
    }

    @Test
    void aSkinThatNeverShowsFails(@TempDir Path dir) {
        long[] now = {0};
        Bot bot = new Bot(config(dir.toFile(), "skin", "wide"), () -> now[0]);
        FakeGame game = new FakeGame();
        game.skin = "minecraft:textures/entity/player/wide/steve.png";
        Report report = run(bot, game, now);

        assertFalse(report.passed());
        assertTrue(report.checks.stream().anyMatch(c -> c.name.equals("skin") && !c.ok));
    }

    @Test
    void aForeignScreenOverTheWorldIsClosedAndReported(@TempDir Path dir) {
        long[] now = {0};
        Bot bot = new Bot(config(dir.toFile()), () -> now[0]);
        FakeGame game = new FakeGame() {
            @Override
            void step() {
                super.step();
                if (tick == 600) {
                    screen = "some.mod.UpdateScreen";
                }
            }
        };
        Report report = run(bot, game, now);

        assertTrue(report.problems.stream().anyMatch(p -> p.contains("some.mod.UpdateScreen")), report.toJson());
    }

    @Test
    void theLoadingScreenOverANewWorldIsNotAProblem(@TempDir Path dir) {
        long[] now = {0};
        Bot bot = new Bot(config(dir.toFile()), () -> now[0]);
        FakeGame game = new FakeGame() {
            @Override
            void step() {
                super.step();
                // The world is there while its terrain still loads behind the loading screen.
                if (ticksToWorld > 0 && ticksToWorld < 30) {
                    world = true;
                }
            }

            @Override
            public void createWorld(String name, long seed) {
                screen = "net.minecraft.client.gui.screens.LevelLoadingScreen";
                ticksToWorld = 300;
            }
        };
        Report report = run(bot, game, now);

        assertTrue(report.problems.isEmpty(), report.toJson());
        assertTrue(report.passed(), report.toJson());
    }

    @Test
    void jsonIsPlainAscii() {
        StringBuilder out = new StringBuilder();
        Json.string(out, "a\"b\\c\ndé");
        assertEquals("\"a\\\"b\\\\c\\nd\\u00e9\"", out.toString());
    }
}
