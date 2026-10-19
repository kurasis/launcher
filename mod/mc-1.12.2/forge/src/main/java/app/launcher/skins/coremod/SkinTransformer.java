package app.launcher.skins.coremod;

import java.util.HashMap;
import java.util.Map;
import net.minecraft.launchwrapper.IClassTransformer;
import org.objectweb.asm.ClassReader;
import org.objectweb.asm.ClassWriter;
import org.objectweb.asm.Opcodes;
import org.objectweb.asm.tree.AbstractInsnNode;
import org.objectweb.asm.tree.ClassNode;
import org.objectweb.asm.tree.InsnList;
import org.objectweb.asm.tree.InsnNode;
import org.objectweb.asm.tree.MethodInsnNode;
import org.objectweb.asm.tree.MethodNode;
import org.objectweb.asm.tree.VarInsnNode;

/**
 * Passes what {@code NetworkPlayerInfo} returns for a player's skin, cape, elytra and model
 * through {@code LauncherSkins} before every return: the 1.12.2 counterpart of the Mixin
 * the newer versions use. No branches are added, so the stack map frames stay valid.
 * Methods are matched by their MCP name (development) or SRG name (a released game).
 */
public final class SkinTransformer implements IClassTransformer {
    private static final String TARGET = "net.minecraft.client.network.NetworkPlayerInfo";
    private static final String INFO = "net/minecraft/client/network/NetworkPlayerInfo";
    private static final String HOOKS = "app/launcher/skins/game/LauncherSkins";
    private static final String LOCATION = "Lnet/minecraft/util/ResourceLocation;";
    private static final String STRING = "Ljava/lang/String;";

    /** Method name (MCP and SRG) → the hook that filters its result. */
    private static final Map<String, String> HOOKED = new HashMap<>();

    static {
        HOOKED.put("getLocationSkin", "skin");
        HOOKED.put("func_178837_g", "skin");
        HOOKED.put("getLocationCape", "cape");
        HOOKED.put("func_178861_h", "cape");
        HOOKED.put("getLocationElytra", "cape");
        HOOKED.put("func_187106_i", "cape");
        HOOKED.put("getSkinType", "model");
        HOOKED.put("func_178851_f", "model");
    }

    @Override
    public byte[] transform(String name, String transformedName, byte[] basicClass) {
        if (basicClass == null || !TARGET.equals(transformedName)) {
            return basicClass;
        }
        ClassNode node = new ClassNode();
        new ClassReader(basicClass).accept(node, 0);
        int hooked = 0;
        for (MethodNode method : node.methods) {
            String hook = HOOKED.get(method.name);
            String type = "model".equals(hook) ? STRING : LOCATION;
            if (hook == null || !method.desc.equals("()" + type)) {
                continue;
            }
            for (AbstractInsnNode insn : method.instructions.toArray()) {
                if (insn.getOpcode() == Opcodes.ARETURN) {
                    InsnList call = new InsnList();
                    call.add(new VarInsnNode(Opcodes.ALOAD, 0));
                    call.add(new MethodInsnNode(Opcodes.INVOKESTATIC, HOOKS, hook,
                            "(" + type + "L" + INFO + ";)" + type, false));
                    method.instructions.insertBefore(insn, call);
                }
            }
            method.maxStack += 1;
            hooked++;
        }
        if (hooked == 0) {
            return basicClass;
        }
        ClassWriter writer = new ClassWriter(0);
        node.accept(writer);
        return writer.toByteArray();
    }
}
