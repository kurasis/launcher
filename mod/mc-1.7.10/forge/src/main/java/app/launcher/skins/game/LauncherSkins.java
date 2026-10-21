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
import java.util.Arrays;
import java.util.HashMap;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import javax.imageio.ImageIO;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.AbstractClientPlayer;
import net.minecraft.client.renderer.texture.DynamicTexture;
import net.minecraft.util.ResourceLocation;
import org.apache.logging.log4j.LogManager;
import org.apache.logging.log4j.Logger;

/**
 * The game's side of Launcher Skins (D-065) for Minecraft 1.7.10: turns loaded PNGs into
 * textures (one per frame) and hands them to {@code AbstractClientPlayer} through the
 * coremod's hooks. The 1.7.10 player model reads a skin in the format before 1.8 (64×32
 * × k), so a square skin is drawn from its top half; texture coordinates are fractions of
 * the texture, so HD skins just work. 1.7.10 has no slim model. Nothing about players is
 * logged.
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
    public static ResourceLocation skin(ResourceLocation vanilla, AbstractClientPlayer player) {
        Worn worn = worn(player);
        ResourceLocation skin = worn == null ? null : worn.skinNow(System.currentTimeMillis());
        return skin != null ? skin : vanilla;
    }

    /** Hook for the cape. */
    public static ResourceLocation cape(ResourceLocation vanilla, AbstractClientPlayer player) {
        Worn worn = worn(player);
        ResourceLocation cape = worn == null ? null : worn.capeNow(System.currentTimeMillis());
        return cape != null ? cape : vanilla;
    }

    /** Hook for whether the cape is drawn: also when only ours is there. */
    public static boolean hasCape(boolean vanilla, AbstractClientPlayer player) {
        if (vanilla) {
            return true;
        }
        Worn worn = worn(player);
        return worn != null && worn.capeFrames != null;
    }

    private static Worn worn(AbstractClientPlayer player) {
        LauncherSkins self = instance;
        GameProfile profile = player == null ? null : player.getGameProfile();
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

    // 1.7.10's mappings leave addScheduledTask with its SRG name, func_152344_a.
    @Override
    public void ready(String key, Profile profile, byte[] skin, byte[] cape) {
        Minecraft.getMinecraft().func_152344_a(() -> {
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
        Minecraft.getMinecraft().func_152344_a(() -> drop(worn.remove(key)));
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
                    if (height * 2 == width) {
                        argb = SkinImage.upgradeLegacy(argb, width);
                    }
                    SkinImage.opaqueInnerLayer(argb, width);
                    // The top half is the format before 1.8, all the 1.7.10 model reads.
                    frameHeight = width / 2;
                    argb = Arrays.copyOf(argb, width * frameHeight);
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
