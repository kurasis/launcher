package app.launcher.skins.core;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;

/** What a launcher account wears: a skin, a cape, or both. */
public final class Profile {
    public final String nick;
    public final Texture skin;
    public final Texture cape;

    Profile(String nick, Texture skin, Texture cape) {
        this.nick = nick;
        this.skin = skin;
        this.cape = cape;
    }

    /**
     * A profile from the service's JSON for the nick asked about, or null if it is broken,
     * names another nick or wears nothing. A broken skin or cape is left out on its own.
     */
    static Profile parse(String asked, JsonElement element) {
        if (element == null || !element.isJsonObject()) {
            return null;
        }
        JsonObject json = element.getAsJsonObject();
        JsonElement nick = json.get("nick");
        if (nick == null || !nick.isJsonPrimitive() || !Nicks.valid(nick.getAsString())
                || !Nicks.key(nick.getAsString()).equals(Nicks.key(asked))) {
            return null;
        }
        Texture skin = Texture.parse(Rules.Kind.SKIN, json.get("skin"));
        Texture cape = Texture.parse(Rules.Kind.CAPE, json.get("cape"));
        return skin == null && cape == null ? null : new Profile(nick.getAsString(), skin, cape);
    }

    public boolean sameAs(Profile other) {
        return other != null && same(skin, other.skin) && same(cape, other.cape);
    }

    private static boolean same(Texture a, Texture b) {
        return a == null ? b == null : a.equals(b);
    }
}
