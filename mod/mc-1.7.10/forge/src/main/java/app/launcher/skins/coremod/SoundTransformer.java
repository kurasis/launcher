package app.launcher.skins.coremod;

import net.minecraft.launchwrapper.IClassTransformer;
import org.objectweb.asm.ClassReader;
import org.objectweb.asm.ClassWriter;
import org.objectweb.asm.Opcodes;
import org.objectweb.asm.tree.ClassNode;
import org.objectweb.asm.tree.MethodInsnNode;
import org.objectweb.asm.tree.MethodNode;

/**
 * Makes {@code SoundManager.reloadSoundSystem} first wait for a sound system that is
 * still starting. Forge reloads the resources twice at start; on a fast machine the
 * second reload comes while the first sound system starts, finds nothing loaded to stop
 * and starts a second one, whose OpenAL context fails, and the game crashes 30 seconds
 * later (UnsatisfiedLinkError in AL10). One static call at the method's start: no
 * branches, so the stack map frames stay valid.
 */
public final class SoundTransformer implements IClassTransformer {
    private static final String TARGET = "net.minecraft.client.audio.SoundManager";
    private static final String HOOKS = "app/launcher/skins/game/SoundStart";

    @Override
    public byte[] transform(String name, String transformedName, byte[] basicClass) {
        if (basicClass == null || !TARGET.equals(transformedName)) {
            return basicClass;
        }
        ClassNode node = new ClassNode();
        new ClassReader(basicClass).accept(node, 0);
        boolean hooked = false;
        for (Object member : node.methods) {
            MethodNode method = (MethodNode) member;
            boolean reload =
                    "reloadSoundSystem".equals(method.name) || "func_148596_a".equals(method.name);
            if (!reload || !"()V".equals(method.desc)) {
                continue;
            }
            method.instructions.insert(
                    new MethodInsnNode(Opcodes.INVOKESTATIC, HOOKS, "await", "()V", false));
            hooked = true;
        }
        if (!hooked) {
            return basicClass;
        }
        ClassWriter writer = new ClassWriter(0);
        node.accept(writer);
        return writer.toByteArray();
    }
}
