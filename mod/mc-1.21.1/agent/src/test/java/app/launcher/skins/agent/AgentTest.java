package app.launcher.skins.agent;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;

import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.Test;
import org.objectweb.asm.ClassReader;
import org.objectweb.asm.ClassWriter;
import org.objectweb.asm.Label;
import org.objectweb.asm.MethodVisitor;
import org.objectweb.asm.Opcodes;
import org.objectweb.asm.Type;
import org.objectweb.asm.tree.AbstractInsnNode;
import org.objectweb.asm.tree.ClassNode;
import org.objectweb.asm.tree.MethodInsnNode;
import org.objectweb.asm.tree.MethodNode;

class AgentTest {
    /** As the obfuscated game has it: PlayerInfo is "a", PlayerSkin is "b". */
    private static final Type HOOK = Type.getMethodType("(Lb;La;)Lb;");

    private static byte[] playerInfo(int getters) {
        ClassWriter writer = new ClassWriter(ClassWriter.COMPUTE_FRAMES | ClassWriter.COMPUTE_MAXS) {
            @Override
            protected String getCommonSuperClass(String a, String b) {
                return "java/lang/Object";
            }
        };
        writer.visit(Opcodes.V21, Opcodes.ACC_PUBLIC, "a", null, "java/lang/Object", null);
        for (int i = 0; i < getters; i++) {
            MethodVisitor m = writer.visitMethod(Opcodes.ACC_PUBLIC, "g" + i, "()Lb;", null, null);
            m.visitCode();
            Label other = new Label();
            m.visitInsn(Opcodes.ICONST_0);
            m.visitJumpInsn(Opcodes.IFEQ, other);
            m.visitInsn(Opcodes.ACONST_NULL);
            m.visitInsn(Opcodes.ARETURN);
            m.visitLabel(other);
            m.visitInsn(Opcodes.ACONST_NULL);
            m.visitInsn(Opcodes.ARETURN);
            m.visitMaxs(0, 0);
            m.visitEnd();
        }
        MethodVisitor s = writer.visitMethod(Opcodes.ACC_PUBLIC | Opcodes.ACC_STATIC, "s", "()Lb;", null, null);
        s.visitCode();
        s.visitInsn(Opcodes.ACONST_NULL);
        s.visitInsn(Opcodes.ARETURN);
        s.visitMaxs(0, 0);
        s.visitEnd();
        writer.visitEnd();
        return writer.toByteArray();
    }

    private static List<String> calls(byte[] bytes, String method) {
        ClassNode node = new ClassNode();
        new ClassReader(bytes).accept(node, 0);
        List<String> calls = new ArrayList<>();
        for (MethodNode m : node.methods) {
            if (m.name.equals(method)) {
                for (AbstractInsnNode insn : m.instructions.toArray()) {
                    if (insn instanceof MethodInsnNode call) {
                        calls.add(call.owner + "." + call.name + call.desc);
                    }
                }
            }
        }
        return calls;
    }

    @Test
    void theOnlyGetterPassesEveryReturnThroughTheHook() {
        byte[] hooked = Agent.hook(playerInfo(1), HOOK);
        assertNotNull(hooked);
        String hook = AgentHooks.class.getName().replace('.', '/') + ".skin" + HOOK.getDescriptor();
        assertEquals(List.of(hook, hook), calls(hooked, "g0"));
        assertEquals(List.of(), calls(hooked, "s"));
    }

    @Test
    void twoCandidatesLeaveTheClassAlone() {
        assertNull(Agent.hook(playerInfo(2), HOOK));
        assertNull(Agent.hook(playerInfo(0), HOOK));
    }

    @Test
    void theHookIsReadFromThisJar() {
        // Unremapped in tests: AgentHooks.skin(PlayerSkin, PlayerInfo) in Mojang names.
        Type hook = Agent.hookType();
        assertNotNull(hook);
        assertEquals("net/minecraft/client/multiplayer/PlayerInfo", hook.getArgumentTypes()[1].getInternalName());
    }
}
