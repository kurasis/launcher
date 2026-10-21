package app.launcher.skins.core;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.google.gson.JsonPrimitive;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.MalformedURLException;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.Collection;
import java.util.HashMap;
import java.util.Locale;
import java.util.Map;

/**
 * The skins.lapi.sh API: profiles by nick and textures by hash. HTTPS only, to the one host
 * it was given, no redirects, short timeouts and a size limit on every answer. Texture
 * addresses are built here from the hash; an address in an answer is never followed.
 */
public final class Api {
    public static final String DEFAULT = "https://skins.lapi.sh";
    /** The launcher passes the address of its service; another launcher gets the default. */
    public static final String PROPERTY = "launcherskins.api";
    public static final int MAX_JSON = 256 * 1024;
    public static final int MAX_NICKS = 50;

    private final String base;
    private final String agent;

    Api(String base, String agent) {
        this.base = base;
        this.agent = agent;
    }

    /** The service the launcher named, or the default if it named none or a bad one. */
    public static Api fromSystem(String version) {
        String agent = "LauncherSkins/" + version;
        String asked = System.getProperty(PROPERTY);
        if (asked != null) {
            String base = check(asked);
            if (base != null) {
                return new Api(base, agent);
            }
        }
        return new Api(DEFAULT, agent);
    }

    /**
     * The address as used, or null if it is not allowed: HTTPS with a host and nothing else
     * (no user, query or path). Plain HTTP only to this computer, for tests.
     */
    static String check(String address) {
        try {
            URL url = new URL(address.trim());
            String scheme = url.getProtocol().toLowerCase(Locale.ROOT);
            String host = url.getHost();
            if (host == null || host.isEmpty() || url.getUserInfo() != null || url.getQuery() != null
                    || url.getRef() != null || !(url.getPath().isEmpty() || url.getPath().equals("/"))) {
                return null;
            }
            boolean local = host.equals("127.0.0.1") || host.equals("localhost") || host.equals("[::1]");
            if (!scheme.equals("https") && !(scheme.equals("http") && local)) {
                return null;
            }
            return scheme + "://" + host + (url.getPort() == -1 ? "" : ":" + url.getPort());
        } catch (MalformedURLException e) {
            return null;
        }
    }

    /** What this nick wears, or null if nothing (or the account does not exist). */
    public Profile profile(String nick) throws IOException {
        if (!Nicks.valid(nick)) {
            return null;
        }
        byte[] body = request("GET", "/v1/profile/" + nick, null, MAX_JSON);
        return body == null ? null : Profile.parse(nick, json(body));
    }

    /** What each of these nicks wears; nicks that wear nothing are left out. */
    public Map<String, Profile> profiles(Collection<String> nicks) throws IOException {
        JsonArray list = new JsonArray();
        for (String nick : nicks) {
            if (Nicks.valid(nick) && list.size() < MAX_NICKS) {
                list.add(new JsonPrimitive(nick));
            }
        }
        Map<String, Profile> found = new HashMap<>();
        if (list.size() == 0) {
            return found;
        }
        JsonObject ask = new JsonObject();
        ask.add("nicks", list);
        byte[] body = request("POST", "/v1/profiles", ask.toString().getBytes(StandardCharsets.UTF_8), MAX_JSON);
        if (body == null) {
            return found;
        }
        JsonElement answer = json(body);
        if (!answer.isJsonObject() || !answer.getAsJsonObject().has("profiles")
                || !answer.getAsJsonObject().get("profiles").isJsonObject()) {
            throw new IOException("unexpected answer");
        }
        JsonObject profiles = answer.getAsJsonObject().getAsJsonObject("profiles");
        for (JsonElement nick : list) {
            String asked = nick.getAsString();
            Profile profile = Profile.parse(asked, profiles.get(asked));
            if (profile != null) {
                found.put(Nicks.key(asked), profile);
            }
        }
        return found;
    }

    /** The PNG of a texture, checked against its hash and its declared size. */
    public byte[] texture(Texture texture) throws IOException {
        byte[] png = request("GET", "/v1/textures/" + texture.hash, null, texture.kind.maxBytes());
        if (png == null) {
            throw new IOException("texture not found");
        }
        if (!matches(texture, png)) {
            throw new IOException("texture does not match its description");
        }
        return png;
    }

    /** Whether these bytes are the texture described: its hash, and a PNG of its size. */
    public static boolean matches(Texture texture, byte[] png) {
        if (png.length > texture.kind.maxBytes() || !sha256(png).equals(texture.hash)) {
            return false;
        }
        int[] size = Png.size(png);
        return size != null && size[0] == texture.width && size[1] == texture.height;
    }

    public static String sha256(byte[] bytes) {
        try {
            byte[] digest = MessageDigest.getInstance("SHA-256").digest(bytes);
            StringBuilder hex = new StringBuilder(64);
            for (byte b : digest) {
                hex.append(Character.forDigit((b >> 4) & 0xF, 16)).append(Character.forDigit(b & 0xF, 16));
            }
            return hex.toString();
        } catch (NoSuchAlgorithmException e) {
            throw new IllegalStateException(e);
        }
    }

    private static JsonElement json(byte[] body) throws IOException {
        try {
            @SuppressWarnings("deprecation") // JsonParser.parseString is newer than 1.8.9's Gson.
            JsonElement parsed = new JsonParser().parse(new String(body, StandardCharsets.UTF_8));
            return parsed;
        } catch (RuntimeException e) {
            throw new IOException("not JSON");
        }
    }

    /** The body of a 200 answer, null for 404 (or 204), an error for anything else. */
    private byte[] request(String method, String path, byte[] body, int limit) throws IOException {
        HttpURLConnection http = (HttpURLConnection) new URL(base + path).openConnection();
        try {
            http.setInstanceFollowRedirects(false);
            http.setUseCaches(false);
            http.setConnectTimeout(5_000);
            http.setReadTimeout(10_000);
            http.setRequestMethod(method);
            http.setRequestProperty("User-Agent", agent);
            if (body != null) {
                http.setDoOutput(true);
                http.setRequestProperty("Content-Type", "application/json");
                http.setFixedLengthStreamingMode(body.length);
                try (OutputStream out = http.getOutputStream()) {
                    out.write(body);
                }
            }
            int status = http.getResponseCode();
            if (status == 404 || status == 204) {
                return null;
            }
            if (status != 200) {
                throw new IOException("answered " + status);
            }
            if (http.getContentLengthLong() > limit) {
                throw new IOException("answer too large");
            }
            try (InputStream in = http.getInputStream()) {
                return read(in, limit);
            }
        } finally {
            http.disconnect();
        }
    }

    static byte[] read(InputStream in, int limit) throws IOException {
        ByteArrayOutputStream out = new ByteArrayOutputStream(Math.min(limit, 64 * 1024));
        byte[] buffer = new byte[16 * 1024];
        int total = 0;
        for (int n; (n = in.read(buffer)) != -1; ) {
            total += n;
            if (total > limit) {
                throw new IOException("answer too large");
            }
            out.write(buffer, 0, n);
        }
        return out.toByteArray();
    }
}
