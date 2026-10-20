package app.launcher.skins.agent;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
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
    private static final String HOOKS = AgentHooks.class.getName().replace('.', '/');

    private static byte[] playerInfo(int getters) {
        ClassWriter writer = new ClassWriter(ClassWriter.COMPUTE_FRAMES | ClassWriter.COMPUTE_MAXS) {
            @Override
            protected String getCommonSuperClass(String a, String b) {
                return "java/lang/Object";
            }
        };
        writer.visit(Opcodes.V1_8, Opcodes.ACC_PUBLIC, "a", null, "java/lang/Object", null);
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
                    if (insn instanceof MethodInsnNode) {
                        MethodInsnNode call = (MethodInsnNode) insn;
                        calls.add(call.owner + "." + call.name + call.desc);
                    }
                }
            }
        }
        return calls;
    }

    private static Agent.Pair pair(String getter) {
        return new Agent.Pair(
                new MethodInsnNode(Opcodes.INVOKEVIRTUAL, "a", getter, "()Lb;", false),
                new MethodInsnNode(Opcodes.INVOKESTATIC, HOOKS, "skin", HOOK.getDescriptor(), false));
    }

    @Test
    void theGetterPassesEveryReturnThroughItsHook() {
        byte[] hooked = Agent.hook(playerInfo(2), Arrays.asList(pair("g1")));
        assertNotNull(hooked);
        String hook = HOOKS + ".skin" + HOOK.getDescriptor();
        assertEquals(Arrays.asList(hook, hook), calls(hooked, "g1"));
        assertEquals(Collections.emptyList(), calls(hooked, "g0"));
        assertEquals(Collections.emptyList(), calls(hooked, "s"));
    }

    @Test
    void aMissingGetterLeavesTheClassAlone() {
        assertNull(Agent.hook(playerInfo(1), Arrays.asList(pair("g0"), pair("g5"))));
        assertNull(Agent.hook(playerInfo(0), Arrays.asList(pair("g0"))));
    }

    @Test
    void theTargetsAreReadFromThisJar() {
        // Unremapped in tests: the getters and hooks in Mojang names.
        List<Agent.Pair> pairs = Agent.targets();
        assertFalse(pairs.isEmpty());
        for (Agent.Pair pair : pairs) {
            assertEquals("net/minecraft/client/multiplayer/PlayerInfo", pair.getter.owner);
            assertEquals(HOOKS, pair.hook.owner);
        }
    }
}
