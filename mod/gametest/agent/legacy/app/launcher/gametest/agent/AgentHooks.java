package app.launcher.gametest.agent;

import app.launcher.gametest.core.Driver;
import app.launcher.gametest.game.GameLayer;
import net.minecraft.client.Minecraft;

/**
 * What the rewritten {@code Minecraft.runTick()} calls first, for releases before 1.13
 * (MCP names). {@link Agent} reads {@link #target} to learn the method's name in the
 * released game.
 */
public final class AgentHooks {
    private static GameLayer layer;

    private AgentHooks() {}

    public static void tick(Minecraft mc) {
        if (layer == null) {
            layer = new GameLayer(mc);
        }
        Driver.tick(layer);
    }

    /**
     * Never called. Its one call names the hooked method: remapping the jar to the
     * released game's names rewrites this call, not strings.
     */
    static void target(Minecraft mc) {
        mc.runTick();
    }
}
