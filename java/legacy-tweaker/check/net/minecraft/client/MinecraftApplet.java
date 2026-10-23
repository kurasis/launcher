package net.minecraft.client;

import java.applet.Applet;
import java.awt.Toolkit;
import java.awt.Window;
import java.awt.event.WindowEvent;

import javax.swing.SwingUtilities;

/** A stand-in for the oldest games: its player closes the window once it shows. */
public class MinecraftApplet extends Applet {
    public void start() {
        new Thread(new Runnable() {
            public void run() {
                try {
                    Thread.sleep(1000);
                } catch (InterruptedException e) {
                    return;
                }
                Window window = SwingUtilities.getWindowAncestor(MinecraftApplet.this);
                Toolkit.getDefaultToolkit().getSystemEventQueue()
                        .postEvent(new WindowEvent(window, WindowEvent.WINDOW_CLOSING));
            }
        }).start();
    }
}
