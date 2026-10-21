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
import net.minecraft.world.WorldSettings;
import net.minecraft.world.WorldType;

/** The test bot's hands in Minecraft 1.8.9 (Forge, MCP names). */
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
        // 1.8.9 loads its resources before the first tick: no overlay to wait for.
        return false;
    }

    @Override
    public boolean onTitle() {
        return mc.currentScreen instanceof GuiMainMenu;
    }

    @Override
    public boolean worldOpen() {
        return mc.theWorld != null && mc.thePlayer != null;
    }

    @Override
    public void createWorld(String name, long seed) {
        WorldSettings settings = new WorldSettings(seed, WorldSettings.GameType.CREATIVE, true, false, WorldType.DEFAULT);
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
        if (mc.theWorld == null) {
            return 0;
        }
        // "MultiplayerChunkCache: 441, 441": the client's chunk count has no getter.
        Matcher number = FIRST_NUMBER.matcher(mc.theWorld.getChunkProvider().makeString());
        return number.find() ? Integer.parseInt(number.group(1)) : 0;
    }

    @Override
    public int entities() {
        return mc.theWorld == null ? 0 : mc.theWorld.loadedEntityList.size();
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
        return mc.thePlayer == null ? 0 : mc.thePlayer.rotationYaw;
    }

    @Override
    public void look(float yaw, float pitch) {
        if (mc.thePlayer != null) {
            mc.thePlayer.rotationYaw = yaw;
            mc.thePlayer.rotationPitch = pitch;
        }
    }

    @Override
    public void command(String command) {
        if (mc.thePlayer != null) {
            mc.thePlayer.sendChatMessage("/" + legacy(command));
        }
    }

    /** The bot's commands in 1.8.9's syntax and names (no @s, no "effect give"). */
    static String legacy(String command) {
        return command
                .replace("effect give @s ", "effect @p ")
                .replace("@s", "@p")
                .replace("type=!minecraft:player", "type=!Player")
                .replace("minecraft:oak_planks", "minecraft:planks")
                .replace("minecraft:iron_golem", "VillagerGolem");
    }

    @Override
    public void camera(int view) {
        mc.gameSettings.thirdPersonView = Math.max(0, Math.min(2, view));
    }

    @Override
    public void flying(boolean flying) {
        EntityPlayerSP player = mc.thePlayer;
        if (player != null && player.capabilities.allowFlying && player.capabilities.isFlying != flying) {
            player.capabilities.isFlying = flying;
            player.sendPlayerAbilities();
        }
    }

    @Override
    public boolean dead() {
        return mc.thePlayer != null && (mc.thePlayer.isDead || mc.thePlayer.getHealth() <= 0);
    }

    @Override
    public void respawn() {
        if (mc.thePlayer != null) {
            mc.thePlayer.respawnPlayer();
            mc.displayGuiScreen(null);
        }
    }

    @Override
    public void screenshot(String fileName) {
        ScreenShotHelper.saveScreenshot(mc.mcDataDir, fileName, mc.displayWidth, mc.displayHeight,
                mc.getFramebuffer());
    }

    @Override
    public void openInventory() {
        if (mc.thePlayer != null) {
            mc.displayGuiScreen(new GuiInventory(mc.thePlayer));
        }
    }

    @Override
    public void closeScreen() {
        mc.displayGuiScreen(null);
    }

    @Override
    public String skinTexture() {
        ResourceLocation skin = mc.thePlayer == null ? null : mc.thePlayer.getLocationSkin();
        return skin == null ? null : skin.toString();
    }

    @Override
    public String capeTexture() {
        ResourceLocation cape = mc.thePlayer == null ? null : mc.thePlayer.getLocationCape();
        return cape == null ? null : cape.toString();
    }

    @Override
    public Boolean slim() {
        return mc.thePlayer == null ? null : "slim".equals(mc.thePlayer.getSkinType());
    }

    @Override
    public void quit() {
        mc.shutdown();
    }
}
