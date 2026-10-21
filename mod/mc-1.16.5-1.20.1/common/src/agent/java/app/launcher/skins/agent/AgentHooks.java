package app.launcher.skins.agent;

import app.launcher.skins.game.LauncherSkins;
import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.PlayerInfo;
import net.minecraft.resources.ResourceLocation;

/**
 * What the rewritten getters of {@code PlayerInfo} call, for Minecraft 1.14.4 to 1.20.1.
 * With no loader there is no entry point, so Launcher Skins starts the first time
 * a skin is asked for: by then the game knows its folder.
 */
public final class AgentHooks {
    private static volatile boolean started;

    private AgentHooks() {}

    public static ResourceLocation skin(ResourceLocation vanilla, PlayerInfo info) {
        start();
        return LauncherSkins.skin(vanilla, info);
    }

    public static ResourceLocation cape(ResourceLocation vanilla, PlayerInfo info) {
        start();
        return LauncherSkins.cape(vanilla, info);
    }

    public static ResourceLocation elytra(ResourceLocation vanilla, PlayerInfo info) {
        start();
        return LauncherSkins.cape(vanilla, info);
    }

    public static String model(String vanilla, PlayerInfo info) {
        start();
        return LauncherSkins.model(vanilla, info);
    }

    /** Never called: {@link Agent} reads which getter each hook follows from here. */
    static void targets(PlayerInfo info) {
        skin(info.getSkinLocation(), info);
        cape(info.getCapeLocation(), info);
        elytra(info.getElytraLocation(), info);
        model(info.getModelName(), info);
    }

    private static void start() {
        if (!started) {
            begin();
        }
    }

    private static synchronized void begin() {
        Minecraft game = Minecraft.getInstance();
        if (started || game == null) {
            return;
        }
        String version = AgentHooks.class.getPackage().getImplementationVersion();
        LauncherSkins.start(game.gameDirectory.toPath(), version == null ? "dev" : version);
        started = true;
    }
}
