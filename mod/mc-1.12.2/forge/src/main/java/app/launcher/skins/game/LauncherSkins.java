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
import java.awt.image.BufferedImage;
import java.io.ByteArrayInputStream;
import java.io.IOException;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import javax.imageio.ImageIO;
import net.minecraft.client.Minecraft;
import net.minecraft.client.network.NetworkPlayerInfo;
import net.minecraft.client.renderer.texture.DynamicTexture;
import net.minecraft.util.ResourceLocation;
import org.apache.logging.log4j.LogManager;
import org.apache.logging.log4j.Logger;

/**
 * The game's side of Launcher Skins (D-065) for Minecraft 1.12.2: turns loaded PNGs into
 * textures (one per frame) and hands them to {@code NetworkPlayerInfo} through the
 * coremod's hooks. The player model reads texture coordinates as fractions of the
 * texture, so an HD texture drawn in place of a 64×64 one just works. Nothing about
 * players is logged.
 */
public final class LauncherSkins implements Skins.Sink {
    public static final String ID = "launcherskins";
    private static final Logger LOG = LogManager.getLogger("Launcher Skins");
    private static volatile LauncherSkins instance;

    private final Skins skins;
    /** Nick key → what that player wears, ready to draw. Written on the main thread. */
    private final Map<String, Worn> worn = new ConcurrentHashMap<>();
    /** Texture hash → its frames and how many players wear it. Main thread only. */
    private final Map<String, Registered> registered = new HashMap<>();

    private static final class Registered {
        final ResourceLocation[] frames;
        int users;

        Registered(ResourceLocation[] frames) {
            this.frames = frames;
        }
    }

    private LauncherSkins(Path gameDir, String version) {
        DiskCache cache = new DiskCache(gameDir.resolve(".launcher-skins"));
        cache.tidy();
        this.skins = new Skins(Api.fromSystem(version), cache, this);
    }

    /** Called once by the mod's entry point. */
    public static synchronized void start(Path gameDir, String version) {
        if (instance == null) {
            instance = new LauncherSkins(gameDir, version);
        }
    }

    /** Hook for the skin's texture: ours when the player wears one, else the game's. */
    public static ResourceLocation skin(ResourceLocation vanilla, NetworkPlayerInfo info) {
        Worn worn = worn(info);
        ResourceLocation skin = worn == null ? null : worn.skinNow(System.currentTimeMillis());
        return skin != null ? skin : vanilla;
    }

    /** Hook for the cape, and for the elytra, which wears the cape. */
    public static ResourceLocation cape(ResourceLocation vanilla, NetworkPlayerInfo info) {
        Worn worn = worn(info);
        ResourceLocation cape = worn == null ? null : worn.capeNow(System.currentTimeMillis());
        return cape != null ? cape : vanilla;
    }

    /** Hook for the model: "slim" or "default". */
    public static String model(String vanilla, NetworkPlayerInfo info) {
        Worn worn = worn(info);
        if (worn == null || worn.skin == null) {
            return vanilla;
        }
        return worn.skin.slim ? "slim" : "default";
    }

    private static Worn worn(NetworkPlayerInfo info) {
        LauncherSkins self = instance;
        GameProfile profile = info == null ? null : info.getGameProfile();
        if (self == null || profile == null) {
            return null;
        }
        String nick = profile.getName();
        if (!Nicks.ours(nick, profile.getId(), profile.getProperties().containsKey("textures"))) {
            return null;
        }
        self.skins.want(nick);
        return self.worn.get(Nicks.key(nick));
    }

    @Override
    public void ready(String key, Profile profile, byte[] skin, byte[] cape) {
        Minecraft.getMinecraft().addScheduledTask(() -> {
            Worn before = worn.get(key);
            ResourceLocation[] skinFrames = profile.skin == null ? null : take(profile.skin, skin);
            ResourceLocation[] capeFrames = profile.cape == null ? null : take(profile.cape, cape);
            worn.put(key, new Worn(skinFrames == null ? null : profile.skin, skinFrames,
                    capeFrames == null ? null : profile.cape, capeFrames));
            drop(before);
        });
    }

    @Override
    public void gone(String key) {
        Minecraft.getMinecraft().addScheduledTask(() -> drop(worn.remove(key)));
    }

    /** The frames of a texture, registered once however many players wear it. */
    private ResourceLocation[] take(Texture texture, byte[] png) {
        Registered known = registered.get(texture.hash);
        if (known == null) {
            ResourceLocation[] frames = register(texture, png);
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
            for (ResourceLocation frame : known.frames) {
                Minecraft.getMinecraft().getTextureManager().deleteTexture(frame);
            }
        }
    }

    /** Decodes a checked PNG and registers each frame as a texture; null if it cannot be. */
    private static ResourceLocation[] register(Texture texture, byte[] png) {
        try {
            BufferedImage image = ImageIO.read(new ByteArrayInputStream(png));
            if (image == null || image.getWidth() != texture.width || image.getHeight() != texture.height) {
                return null;
            }
            int width = texture.width;
            int height = texture.frameHeight();
            ResourceLocation[] frames = new ResourceLocation[texture.frames];
            for (int f = 0; f < texture.frames; f++) {
                int[] argb = image.getRGB(0, f * height, width, height, null, 0, width);
                int frameHeight = height;
                if (texture.kind == Rules.Kind.SKIN) {
                    if (height == 32) {
                        argb = SkinImage.upgradeLegacy(argb);
                        frameHeight = 64;
                    }
                    SkinImage.opaqueInnerLayer(argb, width);
                }
                BufferedImage frame = new BufferedImage(width, frameHeight, BufferedImage.TYPE_INT_ARGB);
                frame.setRGB(0, 0, width, frameHeight, argb, 0, width);
                frames[f] = new ResourceLocation(ID,
                        texture.kind.name().toLowerCase(Locale.ROOT) + "/" + texture.hash + "/" + f);
                Minecraft.getMinecraft().getTextureManager().loadTexture(frames[f], new DynamicTexture(frame));
            }
            return frames;
        } catch (IOException | RuntimeException e) {
            LOG.warn("A texture could not be used: {}", e.getClass().getSimpleName());
            return null;
        }
    }
}
