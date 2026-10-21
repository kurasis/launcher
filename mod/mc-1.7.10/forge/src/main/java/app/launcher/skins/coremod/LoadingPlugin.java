package app.launcher.skins.coremod;

import java.util.Map;
import cpw.mods.fml.relauncher.IFMLLoadingPlugin;

/**
 * FML loads this from the jar manifest before the game starts. It only names the
 * transformer; the mod itself is {@code app.launcher.skins.forge.ForgeEntry}. The sorting
 * index puts the transformer after FML's own, so it sees the game's classes with their
 * deobfuscated class names and SRG method names.
 */
@IFMLLoadingPlugin.MCVersion("1.7.10")
@IFMLLoadingPlugin.Name("Launcher Skins")
@IFMLLoadingPlugin.SortingIndex(1001)
@IFMLLoadingPlugin.TransformerExclusions("app.launcher.skins.coremod.")
public final class LoadingPlugin implements IFMLLoadingPlugin {
    @Override
    public String[] getASMTransformerClass() {
        return new String[] {"app.launcher.skins.coremod.SkinTransformer"};
    }

    @Override
    public String getModContainerClass() {
        return null;
    }

    @Override
    public String getSetupClass() {
        return null;
    }

    @Override
    public void injectData(Map<String, Object> data) {}

    @Override
    public String getAccessTransformerClass() {
        return null;
    }
}
