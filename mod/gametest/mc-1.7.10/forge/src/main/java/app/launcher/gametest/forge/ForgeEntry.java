package app.launcher.gametest.forge;

import app.launcher.gametest.core.Driver;
import app.launcher.gametest.game.GameLayer;
import cpw.mods.fml.common.FMLCommonHandler;
import cpw.mods.fml.common.Mod;
import cpw.mods.fml.common.event.FMLInitializationEvent;
import cpw.mods.fml.common.eventhandler.SubscribeEvent;
import cpw.mods.fml.common.gameevent.TickEvent;
import net.minecraft.client.Minecraft;

/** The bot acts at the start of every client tick (posted on FML's own event bus in 1.7.10). */
@Mod(modid = "launchergametest", name = "Launcher game test", acceptableRemoteVersions = "*",
        useMetadata = true)
public final class ForgeEntry {
    private GameLayer layer;

    @Mod.EventHandler
    public void init(FMLInitializationEvent event) {
        FMLCommonHandler.instance().bus().register(this);
    }

    @SubscribeEvent
    public void tick(TickEvent.ClientTickEvent event) {
        if (event.phase != TickEvent.Phase.START) {
            return;
        }
        if (layer == null) {
            layer = new GameLayer(Minecraft.getMinecraft());
        }
        Driver.tick(layer);
    }
}
