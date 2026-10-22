package net.minecraft.launchwrapper;

import java.io.File;
import java.util.List;

/** Compile-time stand-in for launchwrapper 1.5; the game's own jar is used at run time. */
public interface ITweaker {
    void acceptOptions(List<String> args, File gameDir, File assetsDir, String profile);

    void injectIntoClassLoader(LaunchClassLoader classLoader);

    String getLaunchTarget();

    String[] getLaunchArguments();
}
