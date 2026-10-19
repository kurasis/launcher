package app.launcher.skins.forge;

import app.launcher.skins.game.LauncherSkins;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.common.event.FMLPreInitializationEvent;

/** Forge starts Launcher Skins here; the coremod's hooks do nothing until then. */
@Mod(modid = LauncherSkins.ID, name = "Launcher Skins", clientSideOnly = true,
        acceptableRemoteVersions = "*", useMetadata = true)
public final class ForgeEntry {
    @Mod.EventHandler
    public void preInit(FMLPreInitializationEvent event) {
        LauncherSkins.start(event.getModConfigurationDirectory().toPath().getParent(),
                event.getModMetadata().version);
    }
}
