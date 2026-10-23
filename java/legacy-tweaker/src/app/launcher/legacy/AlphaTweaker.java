package app.launcher.legacy;

import java.awt.AWTEvent;
import java.awt.Frame;
import java.awt.Toolkit;
import java.awt.event.AWTEventListener;
import java.awt.event.WindowEvent;

import net.minecraft.launchwrapper.AlphaVanillaTweaker;
import net.minecraft.launchwrapper.LaunchClassLoader;

/**
 * Mojang's tweaker for the oldest games (Classic to Alpha 1.0), which says when the
 * player closes the game window and makes sure the game then ends.
 *
 * <p>Launchwrapper's frame for these games ends them with code 1 and without a word
 * when its window is closed, the code a failed start ends with too. This tweaker
 * prints {@link #CLOSED} first, so the launcher tells the two apart: the toolkit's
 * listeners hear a window event before the window's own.
 *
 * <p>The exit can hang: launchwrapper's shutdown hook pauses the applet, and Classic's
 * paused loop only sleeps, so its thread no longer handles its windows' messages that
 * the input and display hooks wait on. When the game still runs {@link #GRACE_MS}
 * after the close, this tweaker ends it with the same code. Anything unexpected
 * leaves Mojang's behaviour.
 */
public final class AlphaTweaker extends AlphaVanillaTweaker {
    /** The line the launcher looks for (src-tauri/src/launch/crash.rs). */
    static final String CLOSED = "Launcher: the game window was closed";
    /** How long the game's own exit may take after its window is closed. */
    static final long GRACE_MS = 3000;

    @Override
    public void injectIntoClassLoader(LaunchClassLoader classLoader) {
        super.injectIntoClassLoader(classLoader);
        try {
            Toolkit.getDefaultToolkit().addAWTEventListener(new AWTEventListener() {
                @Override
                public void eventDispatched(AWTEvent event) {
                    if (event.getID() == WindowEvent.WINDOW_CLOSING && event.getSource() instanceof Frame) {
                        System.out.println(CLOSED);
                        endAfterGrace();
                    }
                }
            }, AWTEvent.WINDOW_EVENT_MASK);
        } catch (RuntimeException | LinkageError e) {
            System.out.println("Launcher: the window's close not watched: " + e.getClass().getName());
        }
    }

    /** A daemon thread that halts the game if its exit has not ended it in time. */
    private static void endAfterGrace() {
        Thread watchdog = new Thread(new Runnable() {
            @Override
            public void run() {
                try {
                    Thread.sleep(GRACE_MS);
                } catch (InterruptedException e) {
                    return;
                }
                System.out.println("Launcher: the game did not end after its window closed; ended");
                Runtime.getRuntime().halt(1);
            }
        }, "Launcher exit watchdog");
        watchdog.setDaemon(true);
        watchdog.start();
    }
}
