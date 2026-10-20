package app.launcher.gametest.forge;

import app.launcher.gametest.core.Driver;
import app.launcher.gametest.game.GameLayer;
import net.minecraft.client.Minecraft;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.common.event.FMLInitializationEvent;
import net.minecraftforge.fml.common.eventhandler.SubscribeEvent;
import net.minecraftforge.fml.common.gameevent.TickEvent;

/** The bot acts at the start of every client tick. */
@Mod(modid = "launchergametest", name = "Launcher game test", clientSideOnly = true,
        acceptableRemoteVersions = "*", useMetadata = true)
public final class ForgeEntry {
    private GameLayer layer;

    @Mod.EventHandler
    public void init(FMLInitializationEvent event) {
        MinecraftForge.EVENT_BUS.register(this);
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
