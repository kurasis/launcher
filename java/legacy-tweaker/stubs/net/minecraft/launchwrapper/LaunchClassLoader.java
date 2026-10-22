package net.minecraft.launchwrapper;

import java.net.URL;
import java.net.URLClassLoader;

/** Compile-time stand-in for launchwrapper 1.5; the game's own jar is used at run time. */
public class LaunchClassLoader extends URLClassLoader {
    public LaunchClassLoader(URL[] sources) {
        super(sources);
    }

    public void registerTransformer(String transformerClassName) {
    }
}
