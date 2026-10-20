package app.launcher.gametest.core;

import java.io.File;
import java.util.Locale;
import java.util.Properties;

/**
 * What one test run asks of the bot, from system properties the test harness passes
 * (`-Dlaunchergametest.minutes=5` and so on). Without `launchergametest.out` the bot
 * stays off, so the jar does nothing in a game started by hand.
 */
public final class Config {
    public static final String PREFIX = "launchergametest.";

    /** Where the report and the copy of the screenshots go. */
    public final File out;
    /** How long the bot plays once in the world. */
    public final long playMs;
    /** How long each part of the play lasts. */
    public final long phaseMs;
    public final long seed;
    /** The skin the player should end up with: none, wide or slim. */
    public final String skin;
    /** How many frames that skin has; 1 when it is still. */
    public final int skinFrames;
    /** How many frames the cape has; 0 when the player should wear none. */
    public final int capeFrames;

    private Config(File out, long playMs, long phaseMs, long seed, String skin, int skinFrames, int capeFrames) {
        this.out = out;
        this.playMs = playMs;
        this.phaseMs = phaseMs;
        this.seed = seed;
        this.skin = skin;
        this.skinFrames = skinFrames;
        this.capeFrames = capeFrames;
    }

    /** The run's settings, or null when no test asked for the bot. */
    public static Config from(Properties properties) {
        String out = properties.getProperty(PREFIX + "out");
        if (out == null || out.trim().isEmpty()) {
            return null;
        }
        double minutes = number(properties, "minutes", 5, 0.25, 240);
        double phaseSeconds = number(properties, "phaseSeconds", 30, 5, 600);
        long seed = (long) number(properties, "seed", 20261020, -1e15, 1e15);
        String skin = properties.getProperty(PREFIX + "skin", "none").trim().toLowerCase(Locale.ROOT);
        if (!skin.equals("wide") && !skin.equals("slim")) {
            skin = "none";
        }
        int skinFrames = (int) number(properties, "skinFrames", 1, 1, 32);
        int capeFrames = (int) number(properties, "capeFrames", 0, 0, 16);
        return new Config(new File(out.trim()), (long) (minutes * 60_000), (long) (phaseSeconds * 1000), seed,
                skin, skinFrames, capeFrames);
    }

    private static double number(Properties properties, String key, double fallback, double min, double max) {
        String raw = properties.getProperty(PREFIX + key);
        if (raw == null) {
            return fallback;
        }
        try {
            double value = Double.parseDouble(raw.trim());
            return Double.isNaN(value) ? fallback : Math.max(min, Math.min(max, value));
        } catch (NumberFormatException e) {
            return fallback;
        }
    }
}
