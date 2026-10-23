package app.launcher.skins.agent;

import app.launcher.skins.game.LauncherSkins;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.AbstractClientPlayer;
import net.minecraft.util.ResourceLocation;

/**
 * What the rewritten getters of {@code AbstractClientPlayer} call, for Minecraft 1.7.10
 * (the player holds its skin, no slim model) with no loader (MCP names, remapped to the
 * release's own). Launcher Skins starts the first time a skin is asked for: by then the
 * game knows its folder.
 */
public final class AgentHooks {
    private static volatile boolean started;

    private AgentHooks() {}

    public static ResourceLocation skin(ResourceLocation vanilla, AbstractClientPlayer player) {
        start();
        return LauncherSkins.skin(vanilla, player);
    }

    public static ResourceLocation cape(ResourceLocation vanilla, AbstractClientPlayer player) {
        start();
        return LauncherSkins.cape(vanilla, player);
    }

    public static boolean hasCape(boolean vanilla, AbstractClientPlayer player) {
        start();
        return LauncherSkins.hasCape(vanilla, player);
    }

    /** Never called: {@link Agent} reads which getter each hook follows from here. */
    static void targets(AbstractClientPlayer player) {
        skin(player.getLocationSkin(), player);
        cape(player.getLocationCape(), player);
        hasCape(player.hasCape(), player);
    }

    private static void start() {
        if (!started) {
            begin();
        }
    }

    private static synchronized void begin() {
        Minecraft game = Minecraft.getMinecraft();
        if (started || game == null) {
            return;
        }
        String version = AgentHooks.class.getPackage().getImplementationVersion();
        LauncherSkins.start(game.mcDataDir.toPath(), version == null ? "dev" : version);
        started = true;
    }
}
