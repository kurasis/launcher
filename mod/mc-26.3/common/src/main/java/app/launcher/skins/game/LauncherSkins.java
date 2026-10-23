package app.launcher.skins.game;

import app.launcher.skins.core.Api;
import app.launcher.skins.core.DiskCache;
import app.launcher.skins.core.Nicks;
import app.launcher.skins.core.Profile;
import app.launcher.skins.core.Rules;
import app.launcher.skins.core.SkinImage;
import app.launcher.skins.core.Skins;
import app.launcher.skins.core.Texture;
import com.mojang.authlib.GameProfile;
import com.mojang.blaze3d.platform.NativeImage;
import java.io.IOException;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import net.minecraft.client.Minecraft;
import net.minecraft.core.ClientAsset;
import net.minecraft.resources.Identifier;
import net.minecraft.world.entity.player.PlayerModelType;
import net.minecraft.world.entity.player.PlayerSkin;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * The game's side of Launcher Skins (D-065) for Minecraft 26.3, shared by every loader:
 * turns loaded PNGs into textures (one per frame) and swaps them into a player's skin.
 * Nothing about players is logged.
 */
public final class LauncherSkins implements Skins.Sink {
    public static final String ID = "launcherskins";
    private static final Logger LOG = LoggerFactory.getLogger("Launcher Skins");
    private static LauncherSkins instance;

    private final Skins skins;
    /** Nick key → what that player wears, ready to draw. Written on the render thread. */
    private final Map<String, Worn> worn = new ConcurrentHashMap<>();
    /** Texture hash → its frames and how many players wear it. Render thread only. */
    private final Map<String, Registered> registered = new HashMap<>();

    private static final class Registered {
        final Identifier[] frames;
        int users;

        Registered(Identifier[] frames) {
            this.frames = frames;
        }
    }

    private LauncherSkins(Path gameDir, String version) {
        DiskCache cache = new DiskCache(gameDir.resolve(".launcher-skins"));
        cache.tidy();
        this.skins = new Skins(Api.fromSystem(version), cache, this);
    }

    /** Called once by the loader's entry point. */
    public static void start(Path gameDir, String version) {
        if (instance == null) {
            instance = new LauncherSkins(gameDir, version);
        }
    }

    /** The skin to draw for a player: ours when they wear one, else the game's. */
    public static PlayerSkin skin(GameProfile profile, PlayerSkin vanilla) {
        LauncherSkins self = instance;
        if (self == null || vanilla == null) {
            return vanilla;
        }
        String nick = profile.name();
        if (!Nicks.ours(nick, profile.id(), profile.properties().containsKey("textures"))) {
            return vanilla;
        }
        self.skins.want(nick);
        Worn worn = self.worn.get(Nicks.key(nick));
        if (worn == null) {
            return vanilla;
        }
        long now = System.currentTimeMillis();
        Identifier skin = worn.skinNow(now);
        Identifier cape = worn.capeNow(now);
        return new PlayerSkin(
                skin != null ? new ClientAsset.ResourceTexture(skin, skin) : vanilla.body(),
                cape != null ? new ClientAsset.ResourceTexture(cape, cape) : vanilla.cape(),
                cape != null ? new ClientAsset.ResourceTexture(cape, cape) : vanilla.elytra(),
                skin != null ? (worn.skin.slim ? PlayerModelType.SLIM : PlayerModelType.WIDE) : vanilla.model(),
                vanilla.secure());
    }

    @Override
    public void ready(String key, Profile profile, byte[] skin, byte[] cape) {
        Minecraft.getInstance().execute(() -> {
            Worn before = worn.get(key);
            Identifier[] skinFrames = profile.skin == null ? null : take(profile.skin, skin);
            Identifier[] capeFrames = profile.cape == null ? null : take(profile.cape, cape);
            worn.put(key, new Worn(skinFrames == null ? null : profile.skin, skinFrames,
                    capeFrames == null ? null : profile.cape, capeFrames));
            drop(before);
        });
    }

    @Override
    public void gone(String key) {
        Minecraft.getInstance().execute(() -> drop(worn.remove(key)));
    }

    /** The frames of a texture, registered once however many players wear it. */
    private Identifier[] take(Texture texture, byte[] png) {
        Registered known = registered.get(texture.hash);
        if (known == null) {
            Identifier[] frames = register(texture, png);
            if (frames == null) {
                return null;
            }
            known = new Registered(frames);
            registered.put(texture.hash, known);
        }
        known.users++;
        return known.frames;
    }

    private void drop(Worn worn) {
        if (worn == null) {
            return;
        }
        release(worn.skin);
        release(worn.cape);
    }

    private void release(Texture texture) {
        Registered known = texture == null ? null : registered.get(texture.hash);
        if (known != null && --known.users <= 0) {
            registered.remove(texture.hash);
            for (Identifier frame : known.frames) {
                Minecraft.getInstance().getTextureManager().release(frame);
            }
        }
    }

    /** Decodes a checked PNG and registers each frame as a texture; null if it cannot be. */
    private static Identifier[] register(Texture texture, byte[] png) {
        try (NativeImage image = NativeImage.read(png)) {
            if (image.getWidth() != texture.width || image.getHeight() != texture.height) {
                return null;
            }
            int width = texture.width;
            int height = texture.frameHeight();
            Identifier[] frames = new Identifier[texture.frames];
            for (int f = 0; f < texture.frames; f++) {
                int[] argb = new int[width * height];
                for (int y = 0; y < height; y++) {
                    for (int x = 0; x < width; x++) {
                        argb[y * width + x] = image.getPixel(x, f * height + y);
                    }
                }
                int frameHeight = height;
                if (texture.kind == Rules.Kind.SKIN) {
                    if (height * 2 == width) {
                        argb = SkinImage.upgradeLegacy(argb, width);
                        frameHeight = width;
                    }
                    SkinImage.opaqueInnerLayer(argb, width);
                }
                NativeImage frame = new NativeImage(width, frameHeight, true);
                for (int y = 0; y < frameHeight; y++) {
                    for (int x = 0; x < width; x++) {
                        frame.setPixel(x, y, argb[y * width + x]);
                    }
                }
                frames[f] = Identifier.fromNamespaceAndPath(ID,
                        texture.kind.name().toLowerCase(java.util.Locale.ROOT) + "/" + texture.hash + "/" + f);
                Textures.register(frames[f], frame);
            }
            return frames;
        } catch (IOException | RuntimeException e) {
            LOG.warn("A texture could not be used: {}", e.getClass().getSimpleName());
            return null;
        }
    }
}
