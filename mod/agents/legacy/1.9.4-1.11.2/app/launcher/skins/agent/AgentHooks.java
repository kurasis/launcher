package app.launcher.skins.agent;

import app.launcher.skins.game.LauncherSkins;
import net.minecraft.client.Minecraft;
import net.minecraft.client.network.NetworkPlayerInfo;
import net.minecraft.util.ResourceLocation;

/**
 * What the rewritten getters of {@code NetworkPlayerInfo} call, for Minecraft 1.9.4 to
 * 1.11.2 with no loader (MCP names, remapped to the release's own). Launcher Skins
 * starts the first time a skin is asked for: by then the game knows its folder.
 */
public final class AgentHooks {
    private static volatile boolean started;

    private AgentHooks() {}

    public static ResourceLocation skin(ResourceLocation vanilla, NetworkPlayerInfo info) {
        start();
        return LauncherSkins.skin(vanilla, info);
    }

    public static ResourceLocation cape(ResourceLocation vanilla, NetworkPlayerInfo info) {
        start();
        return LauncherSkins.cape(vanilla, info);
    }

    public static ResourceLocation elytra(ResourceLocation vanilla, NetworkPlayerInfo info) {
        start();
        return LauncherSkins.cape(vanilla, info);
    }

    public static String model(String vanilla, NetworkPlayerInfo info) {
        start();
        return LauncherSkins.model(vanilla, info);
    }

    /** Never called: {@link Agent} reads which getter each hook follows from here. */
    static void targets(NetworkPlayerInfo info) {
        skin(info.getLocationSkin(), info);
        cape(info.getLocationCape(), info);
        elytra(info.getLocationElytra(), info);
        model(info.getSkinType(), info);
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
