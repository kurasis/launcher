package app.launcher.skins.core;

import java.util.Locale;
import java.util.UUID;
import java.util.regex.Pattern;

/** Nicks of launcher accounts, and which players get our skins. */
public final class Nicks {
    private static final Pattern NICK = Pattern.compile("[A-Za-z0-9_]{3,16}");

    private Nicks() {}

    /** Whether this can be a launcher account's nick (3–16 of A–Z a–z 0–9 _). */
    public static boolean valid(String nick) {
        return nick != null && NICK.matcher(nick).matches();
    }

    /** The key a nick is kept under: nicks are unique without regard to case. */
    public static String key(String nick) {
        return nick.toLowerCase(Locale.ROOT);
    }

    /**
     * Whether a player gets our skin. A profile that carries textures (a Microsoft account
     * on an online server) keeps them, and so does any Microsoft account (random, version 4
     * UUIDs): a launcher account named like a Microsoft player never replaces that player's
     * skin. Offline players (version 3 UUIDs) and the launcher's own players (version 5)
     * get ours.
     */
    public static boolean ours(String nick, UUID id, boolean hasTextures) {
        return !hasTextures && id != null && id.version() != 4 && valid(nick);
    }
}
