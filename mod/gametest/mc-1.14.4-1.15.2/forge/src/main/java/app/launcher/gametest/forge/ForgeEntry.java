package app.launcher.gametest.forge;

import app.launcher.gametest.core.Driver;
import app.launcher.gametest.game.GameLayer;
import net.minecraft.client.Minecraft;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.event.TickEvent;
import net.minecraftforge.eventbus.api.SubscribeEvent;
import net.minecraftforge.fml.common.Mod;

/** Forge 1.14.4 and 1.15.2 have no Mixin: the bot acts at the start of every client tick. */
@Mod("launchergametest")
public final class ForgeEntry {
    private GameLayer layer;

    public ForgeEntry() {
        MinecraftForge.EVENT_BUS.register(this);
    }

    @SubscribeEvent
    public void tick(TickEvent.ClientTickEvent event) {
        if (event.phase != TickEvent.Phase.START) {
            return;
        }
        if (layer == null) {
            layer = new GameLayer(Minecraft.getInstance());
        }
        Driver.tick(layer);
    }
}
