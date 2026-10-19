package app.launcher.skins.core;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import java.util.Arrays;
import java.util.regex.Pattern;

/** A skin or cape as skins.lapi.sh describes it. */
public final class Texture {
    private static final Pattern HASH = Pattern.compile("[0-9a-f]{64}");

    public final Rules.Kind kind;
    /** SHA-256 of the PNG, lower-case hex: also its name at the service and on disk. */
    public final String hash;
    public final int width;
    public final int height;
    public final int frames;
    /** One duration for every frame, or one per frame; empty for a single frame. */
    private final int[] frameMs;
    /** Skins only: true for the slim (3-pixel) arms. */
    public final boolean slim;

    Texture(Rules.Kind kind, String hash, int width, int height, int frames, int[] frameMs, boolean slim) {
        this.kind = kind;
        this.hash = hash;
        this.width = width;
        this.height = height;
        this.frames = frames;
        this.frameMs = frameMs.clone();
        this.slim = slim;
    }

    public int[] frameMs() {
        return frameMs.clone();
    }

    /** The frame showing at this moment. */
    public int frameAt(long nowMs) {
        return Frames.at(frames, frameMs, nowMs);
    }

    /** The height of one frame. */
    public int frameHeight() {
        return frames == 1 ? height : height / frames;
    }

    /** A texture from the service's JSON, or null if anything in it breaks the rules. */
    static Texture parse(Rules.Kind kind, JsonElement element) {
        try {
            if (element == null || !element.isJsonObject()) {
                return null;
            }
            JsonObject json = element.getAsJsonObject();
            String hash = json.get("hash").getAsString();
            int width = json.get("width").getAsInt();
            int height = json.get("height").getAsInt();
            int frames = json.get("frames").getAsInt();
            if (!HASH.matcher(hash).matches() || Rules.frames(kind, width, height) != frames) {
                return null;
            }
            int[] ms = new int[0];
            if (frames > 1) {
                JsonArray list = json.getAsJsonArray("frame_ms");
                if (list == null || list.size() > Rules.MAX_FRAMES) {
                    return null;
                }
                ms = new int[list.size()];
                for (int i = 0; i < ms.length; i++) {
                    ms[i] = list.get(i).getAsInt();
                }
                if (!Rules.frameMs(ms, frames)) {
                    return null;
                }
            }
            boolean slim = false;
            if (kind == Rules.Kind.SKIN) {
                String model = json.has("model") ? json.get("model").getAsString() : "default";
                if (!model.equals("default") && !model.equals("slim")) {
                    return null;
                }
                slim = model.equals("slim");
            }
            return new Texture(kind, hash, width, height, frames, ms, slim);
        } catch (RuntimeException e) {
            // Missing fields, wrong types, numbers out of range: not a texture.
            return null;
        }
    }

    @Override
    public boolean equals(Object o) {
        if (!(o instanceof Texture)) {
            return false;
        }
        Texture t = (Texture) o;
        return kind == t.kind && hash.equals(t.hash) && frames == t.frames && slim == t.slim
                && Arrays.equals(frameMs, t.frameMs);
    }

    @Override
    public int hashCode() {
        return hash.hashCode();
    }
}
