package app.launcher.skins.forge;

import app.launcher.skins.game.LauncherSkins;
import cpw.mods.fml.common.Mod;
import cpw.mods.fml.common.event.FMLPreInitializationEvent;

/** Forge starts Launcher Skins here; the coremod's hooks do nothing until then. */
@Mod(modid = LauncherSkins.ID, name = "Launcher Skins", acceptableRemoteVersions = "*",
        useMetadata = true)
public final class ForgeEntry {
    @Mod.EventHandler
    public void preInit(FMLPreInitializationEvent event) {
        LauncherSkins.start(event.getModConfigurationDirectory().toPath().getParent(),
                event.getModMetadata().version);
    }
}
