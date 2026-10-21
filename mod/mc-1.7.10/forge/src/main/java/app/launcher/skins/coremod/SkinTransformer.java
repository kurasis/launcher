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
import org.objectweb.asm.tree.MethodInsnNode;
import org.objectweb.asm.tree.MethodNode;
import org.objectweb.asm.tree.VarInsnNode;

/**
 * Passes what {@code AbstractClientPlayer} returns for a player's skin and cape, and
 * whether it has a cape, through {@code LauncherSkins} before every return: 1.7.10 keeps
 * them on the player, not on a {@code NetworkPlayerInfo}. No branches are added, so the
 * stack map frames stay valid. Methods are matched by their MCP name (development) or SRG
 * name (a released game).
 */
public final class SkinTransformer implements IClassTransformer {
    private static final String TARGET = "net.minecraft.client.entity.AbstractClientPlayer";
    private static final String PLAYER = "net/minecraft/client/entity/AbstractClientPlayer";
    private static final String HOOKS = "app/launcher/skins/game/LauncherSkins";
    private static final String LOCATION = "Lnet/minecraft/util/ResourceLocation;";

    /** Method name (MCP and SRG) → the hook that filters its result. */
    private static final Map<String, String> HOOKED = new HashMap<>();

    static {
        HOOKED.put("getLocationSkin", "skin");
        HOOKED.put("func_110306_p", "skin");
        HOOKED.put("getLocationCape", "cape");
        HOOKED.put("func_110303_q", "cape");
        // hasCape, unnamed in 1.7.10's mappings: whether RenderPlayer draws a cape at all.
        HOOKED.put("func_152122_n", "hasCape");
    }

    @Override
    public byte[] transform(String name, String transformedName, byte[] basicClass) {
        if (basicClass == null || !TARGET.equals(transformedName)) {
            return basicClass;
        }
        ClassNode node = new ClassNode();
        new ClassReader(basicClass).accept(node, 0);
        int hooked = 0;
        // Forge 1.7.10's ASM has no generic types: node.methods is a raw List.
        for (Object member : node.methods) {
            MethodNode method = (MethodNode) member;
            String hook = HOOKED.get(method.name);
            String type = "hasCape".equals(hook) ? "Z" : LOCATION;
            int returns = "hasCape".equals(hook) ? Opcodes.IRETURN : Opcodes.ARETURN;
            if (hook == null || !method.desc.equals("()" + type)) {
                continue;
            }
            for (AbstractInsnNode insn : method.instructions.toArray()) {
                if (insn.getOpcode() == returns) {
                    InsnList call = new InsnList();
                    call.add(new VarInsnNode(Opcodes.ALOAD, 0));
                    call.add(new MethodInsnNode(Opcodes.INVOKESTATIC, HOOKS, hook,
                            "(" + type + "L" + PLAYER + ";)" + type, false));
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
