package app.launcher.gametest.game;

import app.launcher.gametest.core.Game;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import net.minecraft.client.Minecraft;
import net.minecraft.client.entity.EntityPlayerSP;
import net.minecraft.client.gui.GuiChat;
import net.minecraft.client.gui.GuiDownloadTerrain;
import net.minecraft.client.gui.GuiMainMenu;
import net.minecraft.client.gui.GuiScreen;
import net.minecraft.client.gui.GuiScreenWorking;
import net.minecraft.client.gui.inventory.GuiInventory;
import net.minecraft.client.settings.GameSettings;
import net.minecraft.client.settings.KeyBinding;
import net.minecraft.server.integrated.IntegratedServer;
import net.minecraft.util.ResourceLocation;
import net.minecraft.util.ScreenShotHelper;
import net.minecraft.world.GameType;
import net.minecraft.world.WorldSettings;
import net.minecraft.world.WorldType;

/** The test bot's hands in Minecraft 1.12.2 (Forge, MCP names). */
public final class GameLayer implements Game {
    private static final Pattern FIRST_NUMBER = Pattern.compile("(\\d+)");
    private final Minecraft mc;

    public GameLayer(Minecraft mc) {
        this.mc = mc;
    }

    @Override
    public String screen() {
        GuiScreen screen = mc.currentScreen;
        if (screen == null) {
            return null;
        }
        // The bot and the report go by the names of today's screens.
        if (screen instanceof GuiMainMenu) {
            return "TitleScreen";
        }
        if (screen instanceof GuiChat) {
            return "ChatScreen";
        }
        if (screen instanceof GuiScreenWorking) {
            return "LevelLoadingScreen";
        }
        if (screen instanceof GuiDownloadTerrain) {
            return "ReceivingLevelScreen";
        }
        return screen.getClass().getName();
    }

    @Override
    public boolean loading() {
        // 1.12.2 loads its resources before the first tick: no overlay to wait for.
        return false;
    }

    @Override
    public boolean onTitle() {
        return mc.currentScreen instanceof GuiMainMenu;
    }

    @Override
    public boolean worldOpen() {
        return mc.world != null && mc.player != null;
    }

    @Override
    public void createWorld(String name, long seed) {
        WorldSettings settings = new WorldSettings(seed, GameType.CREATIVE, true, false, WorldType.DEFAULT);
        settings.enableCommands();
        mc.launchIntegratedServer(name, name, settings);
    }

    @Override
    public void unattended() {
        mc.gameSettings.pauseOnLostFocus = false;
    }

    @Override
    public int fps() {
        return Minecraft.getDebugFPS();
    }

    @Override
    public int chunks() {
        if (mc.world == null) {
            return 0;
        }
        // "MultiplayerChunkCache: 441, 441": the client's chunk count has no getter.
        Matcher number = FIRST_NUMBER.matcher(mc.world.getChunkProvider().makeString());
        return number.find() ? Integer.parseInt(number.group(1)) : 0;
    }

    @Override
    public int entities() {
        return mc.world == null ? 0 : mc.world.loadedEntityList.size();
    }

    @Override
    public double tickMs() {
        IntegratedServer server = mc.getIntegratedServer();
        if (server == null) {
            return -1;
        }
        long[] times = server.tickTimeArray;
        long total = 0;
        for (long time : times) {
            total += time;
        }
        return times.length == 0 ? -1 : total / (double) times.length / 1_000_000.0;
    }

    @Override
    public void keys(boolean forward, boolean left, boolean sprint, boolean jump) {
        GameSettings options = mc.gameSettings;
        KeyBinding.setKeyBindState(options.keyBindForward.getKeyCode(), forward);
        KeyBinding.setKeyBindState(options.keyBindLeft.getKeyCode(), left);
        KeyBinding.setKeyBindState(options.keyBindSprint.getKeyCode(), sprint);
        KeyBinding.setKeyBindState(options.keyBindJump.getKeyCode(), jump);
    }

    @Override
    public float yaw() {
        return mc.player == null ? 0 : mc.player.rotationYaw;
    }

    @Override
    public void look(float yaw, float pitch) {
        if (mc.player != null) {
            mc.player.rotationYaw = yaw;
            mc.player.rotationPitch = pitch;
        }
    }

    @Override
    public void command(String command) {
        if (mc.player != null) {
            mc.player.sendChatMessage("/" + legacy(command));
        }
    }

    /** The bot's commands in 1.12.2's syntax and names (no @s, no "effect give"). */
    static String legacy(String command) {
        return command
                .replace("effect give @s ", "effect @p ")
                .replace("@s", "@p")
                .replace("type=!minecraft:player", "type=!player")
                .replace("minecraft:oak_planks", "minecraft:planks")
                .replace("minecraft:iron_golem", "minecraft:villager_golem");
    }

    @Override
    public void camera(int view) {
        mc.gameSettings.thirdPersonView = Math.max(0, Math.min(2, view));
    }

    @Override
    public void flying(boolean flying) {
        EntityPlayerSP player = mc.player;
        if (player != null && player.capabilities.allowFlying && player.capabilities.isFlying != flying) {
            player.capabilities.isFlying = flying;
            player.sendPlayerAbilities();
        }
    }

    @Override
    public boolean dead() {
        return mc.player != null && (mc.player.isDead || mc.player.getHealth() <= 0);
    }

    @Override
    public void respawn() {
        if (mc.player != null) {
            mc.player.respawnPlayer();
            mc.displayGuiScreen(null);
        }
    }

    @Override
    public void screenshot(String fileName) {
        ScreenShotHelper.saveScreenshot(mc.gameDir, fileName, mc.displayWidth, mc.displayHeight,
                mc.getFramebuffer());
    }

    @Override
    public void openInventory() {
        if (mc.player != null) {
            mc.displayGuiScreen(new GuiInventory(mc.player));
        }
    }

    @Override
    public void closeScreen() {
        mc.displayGuiScreen(null);
    }

    @Override
    public String skinTexture() {
        ResourceLocation skin = mc.player == null ? null : mc.player.getLocationSkin();
        return skin == null ? null : skin.toString();
    }

    @Override
    public String capeTexture() {
        ResourceLocation cape = mc.player == null ? null : mc.player.getLocationCape();
        return cape == null ? null : cape.toString();
    }

    @Override
    public Boolean slim() {
        return mc.player == null ? null : "slim".equals(mc.player.getSkinType());
    }

    @Override
    public void quit() {
        mc.shutdown();
    }
}
