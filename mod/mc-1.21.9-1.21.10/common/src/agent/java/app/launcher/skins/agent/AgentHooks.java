package app.launcher.skins.agent;

import app.launcher.skins.game.LauncherSkins;
import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.PlayerInfo;
import net.minecraft.world.entity.player.PlayerSkin;

/**
 * What the rewritten {@code PlayerInfo.getSkin()} calls, for Minecraft 1.21.9 to 1.21.11.
 * With no loader there is no entry point, so Launcher Skins starts the first time a skin
 * is asked for: by then the game knows its folder.
 */
public final class AgentHooks {
    private static volatile boolean started;

    private AgentHooks() {}

    public static PlayerSkin skin(PlayerSkin vanilla, PlayerInfo info) {
        if (!started) {
            start();
        }
        return LauncherSkins.skin(info.getProfile(), vanilla);
    }

    /** Never called: {@link Agent} reads which getter each hook follows from here. */
    static void targets(PlayerInfo info) {
        skin(info.getSkin(), info);
    }

    private static synchronized void start() {
        Minecraft game = Minecraft.getInstance();
        if (started || game == null) {
            return;
        }
        String version = AgentHooks.class.getPackage().getImplementationVersion();
        LauncherSkins.start(game.gameDirectory.toPath(), version == null ? "dev" : version);
        started = true;
    }
}
