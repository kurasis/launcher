package app.launcher.gametest.core;

import java.io.File;
import java.io.IOException;
import java.util.HashSet;
import java.util.Locale;
import java.util.Random;
import java.util.Set;
import java.util.function.LongSupplier;

/**
 * Plays the game for a test run: waits for the title screen, creates a world with a fixed
 * seed, then walks, flies, teleports far away, fills the area with mobs and builds,
 * looking at its own skin and cape along the way, and measures the game every second.
 * At the end it checks what it saw, writes the report and closes the game.
 *
 * The bot drives the game through {@link Game} from the start of each client tick.
 */
public final class Bot {
    /** The skins mod names its textures in this namespace. */
    static final String SKINS_NAMESPACE = "launcherskins:";
    static final String WORLD = "gametest";
    static final String[] PHASES = {"skin", "walk", "fly", "far", "mobs", "build"};

    private static final long SECOND = 1_000_000_000L;
    private static final long TITLE_SETTLE = 2 * SECOND;
    /** A screen other than the title this long before the world is reported and passed by. */
    private static final long STUCK = 20 * SECOND;
    private static final long TITLE_TIMEOUT = 10 * 60 * SECOND;
    private static final long WORLD_TIMEOUT = 5 * 60 * SECOND;
    /** A screen the bot did not open, over the world this long, is closed. */
    private static final long FOREIGN_SCREEN = 5 * SECOND;
    private static final int MAX_FRAMES_TRACKED = 64;

    private enum Stage { START, TITLE, CREATING, PLAYING, DONE }

    private final Config config;
    private final LongSupplier clock;
    private final Report report = new Report();
    private final Random random;
    private Stage stage = Stage.START;
    private long started;
    private String lastScreen;
    private long screenSince;
    private boolean stuckShot;
    private final Set<String> closedScreens = new HashSet<>();
    private long creatingSince;
    private long playStart;
    private long lastSample;
    private long lastWrite;
    private int phaseIndex = -1;
    private long phaseStart;
    private Report.Phase phase;
    private int shots;
    private boolean inventoryOpen;
    private long foreignSince;
    private float yawTarget;
    private final Set<String> skinIds = new HashSet<>();
    private final Set<String> capeIds = new HashSet<>();
    private Boolean slimSeen;
    private boolean slimModel = true;
    private final Set<String> done = new HashSet<>();

    public Bot(Config config, LongSupplier clock) {
        this.config = config;
        this.clock = clock;
        this.random = new Random(config.seed);
    }

    public Report report() {
        return report;
    }

    public boolean finished() {
        return stage == Stage.DONE;
    }

    public void tick(Game game) {
        long now = clock.getAsLong();
        switch (stage) {
            case START:
                started = now;
                screenSince = now;
                game.unattended();
                stage = Stage.TITLE;
                break;
            case TITLE:
                title(game, now);
                break;
            case CREATING:
                creating(game, now);
                break;
            case PLAYING:
                playing(game, now);
                break;
            default:
                break;
        }
        if (stage != Stage.DONE && now - lastWrite >= 30 * SECOND) {
            lastWrite = now;
            write();
        }
    }

    private void title(Game game, long now) {
        if (now - started > TITLE_TIMEOUT) {
            report.problem("the game did not reach a usable screen within 10 minutes");
            finish(game, now);
            return;
        }
        if (game.loading()) {
            screenSince = now;
            return;
        }
        String screen = game.screen();
        if (screen != null && !screen.equals(lastScreen)) {
            // In the order last seen, so the title shown again after a closed screen comes last.
            report.screensBeforeWorld.remove(screen);
            if (report.screensBeforeWorld.size() < 20) {
                report.screensBeforeWorld.add(screen);
            }
        }
        if (screen == null ? lastScreen != null : !screen.equals(lastScreen)) {
            lastScreen = screen;
            screenSince = now;
        }
        boolean title = game.onTitle();
        if (title && report.titleAfterMs < 0) {
            report.titleAfterMs = ms(now - started);
        }
        long steady = now - screenSince;
        if (title && steady >= TITLE_SETTLE) {
            create(game, now);
        } else if (!title && steady >= STUCK) {
            // A mod's own notice (a welcome or "install this too") is closed once, as a player
            // would; the game's and the loaders' screens are not.
            if (screen != null && modScreen(screen) && closedScreens.add(screen)) {
                report.problem("closed a mod's screen before the title: " + screen);
                game.closeScreen();
                screenSince = now;
                return;
            }
            if (!stuckShot) {
                stuckShot = true;
                shot(game, "stuck");
                report.problem("before the title screen the game stayed on " + (screen == null ? "no screen" : screen));
            }
            create(game, now);
        }
    }

    private void create(Game game, long now) {
        report.worldAfterMs = ms(now - started);
        creatingSince = now;
        stage = Stage.CREATING;
        game.createWorld(WORLD, config.seed);
    }

    private void creating(Game game, long now) {
        if (game.worldOpen() && game.screen() == null) {
            report.worldLoadMs = ms(now - creatingSince);
            playStart = now;
            lastSample = now;
            stage = Stage.PLAYING;
            return;
        }
        if (now - creatingSince > WORLD_TIMEOUT) {
            shot(game, "no-world");
            report.problem("the world did not open within 5 minutes");
            finish(game, now);
        }
    }

    private void playing(Game game, long now) {
        long elapsed = now - playStart;
        if (elapsed >= config.playMs * 1_000_000L) {
            finish(game, now);
            return;
        }
        if (!game.worldOpen()) {
            report.problem("the player left the world during play");
            finish(game, now);
            return;
        }
        if (game.dead()) {
            report.deaths++;
            game.respawn();
            return;
        }
        String screen = game.screen();
        if (screen != null && !inventoryOpen && !ownScreen(screen)) {
            foreign(game, now, screen);
        } else {
            foreignSince = 0;
        }
        watchSkin(game);
        long phaseLen = config.phaseMs * 1_000_000L;
        int index = (int) (elapsed / phaseLen);
        if (index != phaseIndex) {
            endPhase(game);
            phaseIndex = index;
            phaseStart = playStart + index * phaseLen;
            phase = new Report.Phase(PHASES[index % PHASES.length]);
            report.phases.add(phase);
            done.clear();
            startPhase(game);
        }
        if (now - lastSample >= SECOND) {
            lastSample = now;
            sample(game);
        }
        phaseTick(game, ms(now - phaseStart));
    }

    /** Chat, and the game's own screens while terrain loads (after a far teleport). */
    /** Packages of the game's and the loaders' screens. */
    private static final String[] GAME_PACKAGES = {"net.minecraft.", "com.mojang.", "net.minecraftforge.",
        "net.neoforged.", "net.fabricmc.", "org.quiltmc.", "cpw.mods."};

    /**
     * Whether a screen comes from a mod: a class in a package of its own. Obfuscated game
     * classes have no package. The layers may add the screen's title after a space.
     */
    static boolean modScreen(String screen) {
        String name = screen.split(" ", 2)[0];
        if (name.indexOf('.') < 0) {
            return false;
        }
        for (String prefix : GAME_PACKAGES) {
            if (name.startsWith(prefix)) {
                return false;
            }
        }
        return true;
    }

    private static boolean ownScreen(String screen) {
        String name = screen.toLowerCase(Locale.ROOT);
        return name.contains("chat") || name.contains("levelloading") || name.contains("receivinglevel")
                || name.contains("progress");
    }

    private void foreign(Game game, long now, String screen) {
        if (foreignSince == 0) {
            foreignSince = now;
        } else if (now - foreignSince >= FOREIGN_SCREEN) {
            report.problem("a screen opened over the world: " + screen);
            shot(game, "screen");
            game.closeScreen();
            foreignSince = 0;
        }
    }

    private void sample(Game game) {
        int fps = game.fps();
        Runtime runtime = Runtime.getRuntime();
        double usedMb = (runtime.totalMemory() - runtime.freeMemory()) / (1024.0 * 1024.0);
        report.fps.add(fps);
        report.memoryMb.add(usedMb);
        phase.fps.add(fps);
        phase.chunks.add(game.chunks());
        phase.entities.add(game.entities());
        double tick = game.tickMs();
        if (tick >= 0) {
            phase.tickMs.add(tick);
        }
    }

    private void watchSkin(Game game) {
        String skin = game.skinTexture();
        if (skin != null && skin.startsWith(SKINS_NAMESPACE)) {
            if (report.skinAfterMs < 0) {
                report.skinAfterMs = ms(clock.getAsLong() - playStart);
            }
            if (skinIds.size() < MAX_FRAMES_TRACKED) {
                skinIds.add(skin);
            }
            slimModel = game.hasSlimModel();
            Boolean slim = game.slim();
            if (slim != null) {
                slimSeen = slim;
            }
        }
        String cape = game.capeTexture();
        if (cape != null && cape.startsWith(SKINS_NAMESPACE) && capeIds.size() < MAX_FRAMES_TRACKED) {
            capeIds.add(cape);
        }
    }

    private void startPhase(Game game) {
        switch (phase.name) {
            case "skin":
                game.command("gamemode creative");
                game.command("time set 1000");
                // Room around the player, so grass does not hide the skin on the screenshots.
                game.command("fill ~-4 ~ ~-4 ~4 ~3 ~4 minecraft:air");
                game.camera(2);
                break;
            case "walk":
                game.command("gamemode survival");
                game.command("effect give @s minecraft:resistance 100000 4 true");
                game.command("effect give @s minecraft:saturation 100000 0 true");
                yawTarget = game.yaw();
                break;
            case "fly":
                game.command("gamemode creative");
                game.flying(true);
                break;
            case "far":
                game.command("gamemode creative");
                break;
            case "mobs":
                game.command("time set 18000");
                game.command("weather thunder");
                break;
            default:
                break;
        }
    }

    private void endPhase(Game game) {
        if (phase == null) {
            return;
        }
        game.keys(false, false, false, false);
        game.camera(0);
        if (inventoryOpen) {
            game.closeScreen();
            inventoryOpen = false;
        }
        if (phase.name.equals("fly")) {
            game.flying(false);
        }
        if (phase.name.equals("mobs")) {
            game.command("kill @e[type=!minecraft:player]");
            game.command("time set 1000");
            game.command("weather clear");
        }
    }

    /** One tick of the current phase, `t` milliseconds into it. */
    private void phaseTick(Game game, long t) {
        switch (phase.name) {
            case "skin":
                game.look(game.yaw() + 1.5f, 10);
                at(t, 4000, () -> shot(game, "skin-front"));
                at(t, 6000, () -> game.camera(1));
                at(t, 9000, () -> shot(game, "skin-back"));
                at(t, 12000, () -> game.camera(0));
                break;
            case "walk":
                if (t / 3000 != (t - 50) / 3000) {
                    yawTarget += (random.nextFloat() - 0.5f) * 120;
                }
                game.look(approach(game.yaw(), yawTarget), 5);
                game.keys(true, false, true, t % 1500 < 150);
                at(t, 15000, () -> shot(game, "walk"));
                break;
            case "fly":
                game.flying(true);
                game.look(game.yaw() + 0.4f, t % 8000 < 4000 ? -8 : 8);
                game.keys(true, false, true, t % 8000 < 2000);
                at(t, 15000, () -> shot(game, "fly"));
                break;
            case "far":
                for (int i = 0; i < 5; i++) {
                    final int n = i;
                    at(t, 500 + i * 6000L, () -> {
                        int x = (random.nextInt(8000) - 4000) * (n + 1) / 2;
                        int z = (random.nextInt(8000) - 4000) * (n + 1) / 2;
                        game.command("spreadplayers " + x + " " + z + " 0 1 false @s");
                    });
                }
                game.look(game.yaw() + 2f, 0);
                at(t, 20000, () -> shot(game, "far"));
                break;
            case "mobs":
                at(t, 1000, () -> {
                    String[] mobs = {"zombie", "zombie", "zombie", "skeleton", "skeleton", "spider", "creeper",
                        "villager", "villager", "iron_golem", "cow", "sheep", "pig", "chicken", "horse"};
                    for (int i = 0; i < mobs.length; i++) {
                        double angle = i * Math.PI * 2 / mobs.length;
                        game.command(String.format(Locale.ROOT, "summon minecraft:%s ~%.1f ~1 ~%.1f",
                                mobs[i], Math.cos(angle) * 6, Math.sin(angle) * 6));
                    }
                });
                game.look(game.yaw() + 1f, 15);
                at(t, 6000, () -> game.camera(2));
                at(t, 10000, () -> shot(game, "mobs"));
                at(t, 13000, () -> game.camera(0));
                break;
            case "build":
                at(t, 500, () -> {
                    game.command("fill ~3 ~ ~3 ~9 ~6 ~9 minecraft:glass hollow");
                    game.command("fill ~-9 ~ ~3 ~-3 ~4 ~9 minecraft:oak_planks");
                    game.command("setblock ~ ~ ~5 minecraft:torch");
                    game.command("give @s minecraft:diamond_sword");
                    game.command("give @s minecraft:oak_planks 64");
                });
                at(t, 4000, () -> {
                    game.openInventory();
                    inventoryOpen = true;
                });
                at(t, 7000, () -> shot(game, "inventory"));
                at(t, 9000, () -> {
                    game.closeScreen();
                    inventoryOpen = false;
                });
                game.look(game.yaw() + 1f, 20);
                at(t, 20000, () -> {
                    game.command("fill ~3 ~ ~3 ~9 ~6 ~9 minecraft:air");
                    game.command("fill ~-9 ~ ~3 ~-3 ~4 ~9 minecraft:air");
                });
                break;
            default:
                break;
        }
    }

    /** Runs `action` once in this phase, on the first tick at or after `atMs`. */
    private void at(long t, long atMs, Runnable action) {
        String key = phase.name + "@" + atMs;
        if (t >= atMs && done.add(key)) {
            action.run();
        }
    }

    private static float approach(float yaw, float target) {
        float diff = target - yaw;
        return yaw + Math.max(-4f, Math.min(4f, diff));
    }

    private void shot(Game game, String label) {
        if (shots >= 99) {
            return;
        }
        String name = String.format(Locale.ROOT, "gametest-%02d-%s.png", ++shots, label);
        try {
            game.screenshot(name);
            report.screenshots.add(name);
        } catch (RuntimeException e) {
            report.problem("screenshot failed: " + e.getClass().getSimpleName());
        }
    }

    private void finish(Game game, long now) {
        if (stage == Stage.PLAYING) {
            endPhase(game);
            report.playedMs = ms(now - playStart);
        }
        judge();
        report.finished = true;
        stage = Stage.DONE;
        write();
        game.quit();
    }

    /** After repeated errors: what was seen so far is reported unfinished and the game closed. */
    void giveUp(Game game) {
        stage = Stage.DONE;
        try {
            judge();
        } catch (RuntimeException e) {
            report.problem("checks failed: " + e.getClass().getName());
        }
        write();
        game.quit();
    }

    /** The checks: reached the title, played the whole time, wears what the test gave it. */
    void judge() {
        boolean titleFirst = !report.screensBeforeWorld.isEmpty()
                && report.screensBeforeWorld.get(report.screensBeforeWorld.size() - 1).endsWith("TitleScreen")
                && !stuckShot;
        report.check("title-screen", titleFirst,
                titleFirst ? "" : "screens before the world: " + report.screensBeforeWorld);
        boolean world = report.worldLoadMs >= 0;
        report.check("world", world, world ? "" : "no world");
        boolean played = report.playedMs >= config.playMs - 2000;
        report.check("played", played, "played " + report.playedMs / 1000 + " s of " + config.playMs / 1000);
        if (!world) {
            return;
        }
        report.skinFramesSeen = skinIds.size();
        report.capeFramesSeen = capeIds.size();
        if (config.skin.equals("none")) {
            report.check("skin", skinIds.isEmpty(), skinIds.isEmpty() ? "" : "a launcher skin was drawn");
        } else {
            boolean seen = !skinIds.isEmpty();
            boolean slim = config.skin.equals("slim");
            // A game with one model draws every skin with it.
            boolean model = !slimModel || slimSeen != null && slimSeen == slim;
            report.check("skin", seen && model,
                    !seen ? "the launcher skin was never drawn" : model ? "" : "drawn with the wrong model");
            boolean frames = config.skinFrames == 1 ? skinIds.size() == 1 : skinIds.size() >= 2;
            report.check("skin-frames", !seen || frames,
                    "frames seen " + skinIds.size() + " of " + config.skinFrames);
        }
        if (config.capeFrames == 0) {
            report.check("cape", capeIds.isEmpty(), capeIds.isEmpty() ? "" : "a launcher cape was drawn");
        } else {
            boolean seen = !capeIds.isEmpty();
            boolean frames = config.capeFrames == 1 ? capeIds.size() == 1 : capeIds.size() >= 2;
            report.check("cape", seen && frames,
                    !seen ? "the launcher cape was never drawn"
                            : "frames seen " + capeIds.size() + " of " + config.capeFrames);
        }
    }

    private void write() {
        try {
            report.write(config.out);
        } catch (IOException | RuntimeException e) {
            // The harness reports a missing report; nothing else to do from here.
        }
    }

    private static long ms(long nanos) {
        return nanos / 1_000_000L;
    }

    /** The folder the bot writes to, for the version layer's screenshots. */
    public File out() {
        return config.out;
    }
}
