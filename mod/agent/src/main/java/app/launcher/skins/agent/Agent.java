package app.launcher.skins.agent;

import java.io.IOException;
import java.io.InputStream;
import java.lang.instrument.ClassFileTransformer;
import java.lang.instrument.Instrumentation;
import java.security.ProtectionDomain;
import java.util.ArrayList;
import java.util.List;
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
 * Launcher Skins (D-065) without a mod loader, for the releases with obfuscated names
 * (before 26.1). The launcher adds {@code -javaagent:} to the game's command line; this
 * rewrites {@code PlayerInfo}'s skin getters so every value they return passes through a
 * hook in {@link AgentHooks}, as the loaders' Mixin does.
 *
 * <p>The jar is remapped to the released game's names, which rewrites calls but not
 * strings. So the targets are read from {@code AgentHooks.targets}: each getter called
 * there is followed by a call to its hook, {@code hook(getter(), info)}, and both names
 * are the game's own after remapping.
 */
public final class Agent {
    private Agent() {}

    /** A getter of the game's and the hook its result passes through. */
    static final class Pair {
        final MethodInsnNode getter;
        final MethodInsnNode hook;

        Pair(MethodInsnNode getter, MethodInsnNode hook) {
            this.getter = getter;
            this.hook = hook;
        }
    }

    public static void premain(String args, Instrumentation instrumentation) {
        List<Pair> pairs = targets();
        if (pairs.isEmpty()) {
            System.err.println("Launcher Skins: the agent's targets were not found; the game runs unchanged");
            return;
        }
        instrumentation.addTransformer(new Transformer(pairs));
    }

    /** The pairs in {@code AgentHooks.targets}, as this jar has them. */
    static List<Pair> targets() {
        List<Pair> pairs = new ArrayList<>();
        String hooks = AgentHooks.class.getName().replace('.', '/');
        try (InputStream in = Agent.class.getClassLoader().getResourceAsStream(hooks + ".class")) {
            if (in == null) {
                return pairs;
            }
            ClassNode node = new ClassNode();
            new ClassReader(in).accept(node, ClassReader.SKIP_DEBUG | ClassReader.SKIP_FRAMES);
            for (MethodNode method : node.methods) {
                if (!method.name.equals("targets")) {
                    continue;
                }
                MethodInsnNode getter = null;
                for (AbstractInsnNode insn : method.instructions.toArray()) {
                    if (!(insn instanceof MethodInsnNode)) {
                        continue;
                    }
                    MethodInsnNode call = (MethodInsnNode) insn;
                    if (call.getOpcode() == Opcodes.INVOKEVIRTUAL && call.desc.startsWith("()")) {
                        getter = call;
                    } else if (call.getOpcode() == Opcodes.INVOKESTATIC && call.owner.equals(hooks)
                            && getter != null && shaped(call.desc, getter)) {
                        pairs.add(new Pair(getter, call));
                        getter = null;
                    }
                }
            }
        } catch (IOException e) {
            pairs.clear();
        }
        return pairs;
    }

    /** Whether a hook takes the getter's result and its object and returns the same type. */
    private static boolean shaped(String hook, MethodInsnNode getter) {
        String value = getter.desc.substring(2);
        return hook.equals("(" + value + "L" + getter.owner + ";)" + value);
    }

    static final class Transformer implements ClassFileTransformer {
        private final List<Pair> pairs;
        private final String target;

        Transformer(List<Pair> pairs) {
            this.pairs = pairs;
            this.target = pairs.get(0).getter.owner;
        }

        @Override
        public byte[] transform(ClassLoader loader, String name, Class<?> redefined,
                ProtectionDomain domain, byte[] bytes) {
            if (!target.equals(name)) {
                return null;
            }
            try {
                return hook(bytes, pairs);
            } catch (RuntimeException e) {
                // The game runs unchanged rather than not at all.
                System.err.println("Launcher Skins: PlayerInfo left unchanged (" + e.getClass().getSimpleName() + ")");
                return null;
            }
        }
    }

    /**
     * The class with each paired getter passing every result through its hook; null when
     * a getter is missing, so the class is left as it is. No branches are added, so the
     * stack map frames stay valid.
     */
    static byte[] hook(byte[] bytes, List<Pair> pairs) {
        ClassNode node = new ClassNode();
        new ClassReader(bytes).accept(node, 0);
        for (Pair pair : pairs) {
            MethodNode found = null;
            for (MethodNode method : node.methods) {
                if ((method.access & Opcodes.ACC_STATIC) == 0 && method.name.equals(pair.getter.name)
                        && method.desc.equals(pair.getter.desc)) {
                    found = method;
                }
            }
            if (found == null) {
                return null;
            }
            for (AbstractInsnNode insn : found.instructions.toArray()) {
                if (insn.getOpcode() == Opcodes.ARETURN) {
                    InsnList call = new InsnList();
                    call.add(new VarInsnNode(Opcodes.ALOAD, 0));
                    call.add(new MethodInsnNode(Opcodes.INVOKESTATIC, pair.hook.owner, pair.hook.name,
                            pair.hook.desc, false));
                    found.instructions.insertBefore(insn, call);
                }
            }
            found.maxStack += 1;
        }
        ClassWriter writer = new ClassWriter(0);
        node.accept(writer);
        return writer.toByteArray();
    }
}
