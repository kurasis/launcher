package app.launcher.skins.agent;

import app.launcher.skins.game.LauncherSkins;
import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.PlayerInfo;
import net.minecraft.client.resources.PlayerSkin;

/**
 * What the rewritten {@code PlayerInfo.getSkin()} calls. With no loader there is no
 * entry point, so Launcher Skins starts the first time a skin is asked for: by then the
 * game knows its folder. {@link Agent} reads this method's descriptor to find its target.
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
