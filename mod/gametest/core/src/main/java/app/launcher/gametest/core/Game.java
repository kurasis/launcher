package app.launcher.gametest.core;

/**
 * What the bot needs from one game version. Each version's layer implements it in
 * that version's names; the bot itself never touches a game class. Every method is
 * called on the game's client thread, from the start of a client tick.
 */
public interface Game {
    /** The class name of the open screen, or null when none is open. */
    String screen();

    /** Whether the game's resource loading overlay is still up. */
    boolean loading();

    /** Whether the open screen is the title screen. */
    boolean onTitle();

    /** Whether a world is open and the player is in it. */
    boolean worldOpen();

    /** Starts creating and opening a new singleplayer world with commands allowed. */
    void createWorld(String name, long seed);

    /**
     * Settings for a game nobody is sitting at: no pause when the window loses focus,
     * no lower frame rate for an idle player.
     */
    void unattended();

    int fps();

    /** Chunks the client holds, entities the client sees. */
    int chunks();

    int entities();

    /** The integrated server's average tick, in milliseconds; negative if unknown. */
    double tickMs();

    void keys(boolean forward, boolean left, boolean sprint, boolean jump);

    float yaw();

    void look(float yaw, float pitch);

    /** A command without its slash, sent as the player. */
    void command(String command);

    /** 0 first person, 1 third person from behind, 2 third person from the front. */
    void camera(int view);

    void flying(boolean flying);

    boolean dead();

    void respawn();

    /** Saves a screenshot under the game's screenshots folder with this file name. */
    void screenshot(String fileName);

    void openInventory();

    void closeScreen();

    /** The texture the game draws the player's skin with; null if unknown. */
    String skinTexture();

    /** The texture of the player's cape; null when they wear none. */
    String capeTexture();

    /** Whether the player is drawn with the slim model; null if unknown. */
    Boolean slim();

    /** False before 1.8: one player model, so every skin is drawn wide. */
    default boolean hasSlimModel() {
        return true;
    }

    /** Asks the game to close normally. */
    void quit();
}
