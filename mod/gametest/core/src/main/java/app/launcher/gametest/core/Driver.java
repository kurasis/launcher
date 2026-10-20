package app.launcher.gametest.core;

/**
 * The one bot of this game, started on the first client tick when a test asked for it.
 * An error inside the bot is written into the report instead of stopping the game; after
 * a few the bot gives up and the game closes, so a run never hangs on a broken bot.
 */
public final class Driver {
    private static final int MAX_ERRORS = 20;
    private static boolean checked;
    private static Bot bot;
    private static int errors;

    private Driver() {
    }

    public static void tick(Game game) {
        if (!checked) {
            checked = true;
            Config config = Config.from(System.getProperties());
            if (config != null) {
                bot = new Bot(config, System::nanoTime);
            }
        }
        if (bot == null || bot.finished()) {
            return;
        }
        try {
            bot.tick(game);
        } catch (RuntimeException | LinkageError e) {
            errors++;
            StackTraceElement[] trace = e.getStackTrace();
            String where = trace.length > 0 ? " at " + trace[0].getClassName() + "." + trace[0].getMethodName() : "";
            bot.report().problem("bot error: " + e.getClass().getName() + where);
            if (errors >= MAX_ERRORS) {
                bot.report().problem("the bot stopped after " + MAX_ERRORS + " errors");
                Bot broken = bot;
                bot = null;
                broken.giveUp(game);
            }
        }
    }
}
