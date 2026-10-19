package app.launcher.skins.agent;

import java.io.IOException;
import java.io.InputStream;
import java.lang.instrument.ClassFileTransformer;
import java.lang.instrument.Instrumentation;
import java.security.ProtectionDomain;
import org.objectweb.asm.ClassReader;
import org.objectweb.asm.ClassVisitor;
import org.objectweb.asm.ClassWriter;
import org.objectweb.asm.MethodVisitor;
import org.objectweb.asm.Opcodes;
import org.objectweb.asm.Type;
import org.objectweb.asm.tree.AbstractInsnNode;
import org.objectweb.asm.tree.ClassNode;
import org.objectweb.asm.tree.InsnList;
import org.objectweb.asm.tree.MethodInsnNode;
import org.objectweb.asm.tree.MethodNode;
import org.objectweb.asm.tree.VarInsnNode;

/**
 * Launcher Skins (D-065) for Minecraft 1.21.1 without a mod loader. The launcher adds
 * {@code -javaagent:} to the game's command line; this rewrites {@code PlayerInfo.getSkin()}
 * so every value it returns passes through {@link AgentHooks#skin}, as the loaders' Mixin
 * does.
 *
 * <p>The released game's names are obfuscated, and the jar is remapped to them, which
 * changes types in descriptors but not strings. So the target is read from the remapped
 * descriptor of {@code AgentHooks.skin(PlayerSkin, PlayerInfo)}: the class is its second
 * parameter, and {@code getSkin()} is that class's only instance method that takes nothing
 * and returns a {@code PlayerSkin}.
 */
public final class Agent {
    private Agent() {}

    public static void premain(String args, Instrumentation instrumentation) {
        Type hook = hookType();
        if (hook == null) {
            System.err.println("Launcher Skins: the agent's hook was not found; the game runs unchanged");
            return;
        }
        instrumentation.addTransformer(new Transformer(hook));
    }

    /** The descriptor of {@code AgentHooks.skin}, as the remapped jar has it. */
    static Type hookType() {
        String own = AgentHooks.class.getName().replace('.', '/') + ".class";
        try (InputStream in = Agent.class.getClassLoader().getResourceAsStream(own)) {
            if (in == null) {
                return null;
            }
            Type[] found = new Type[1];
            new ClassReader(in).accept(new ClassVisitor(Opcodes.ASM9) {
                @Override
                public MethodVisitor visitMethod(int access, String name, String desc, String sig, String[] ex) {
                    if (name.equals("skin") && (access & Opcodes.ACC_STATIC) != 0) {
                        found[0] = Type.getMethodType(desc);
                    }
                    return null;
                }
            }, ClassReader.SKIP_CODE);
            Type hook = found[0];
            boolean shaped = hook != null && hook.getArgumentTypes().length == 2
                    && hook.getArgumentTypes()[0].equals(hook.getReturnType());
            return shaped ? hook : null;
        } catch (IOException e) {
            return null;
        }
    }

    static final class Transformer implements ClassFileTransformer {
        private final Type hook;
        private final String target;

        Transformer(Type hook) {
            this.hook = hook;
            this.target = hook.getArgumentTypes()[1].getInternalName();
        }

        @Override
        public byte[] transform(ClassLoader loader, String name, Class<?> redefined,
                ProtectionDomain domain, byte[] bytes) {
            if (!target.equals(name)) {
                return null;
            }
            try {
                return hook(bytes, hook);
            } catch (RuntimeException e) {
                // The game runs unchanged rather than not at all.
                System.err.println("Launcher Skins: PlayerInfo left unchanged (" + e.getClass().getSimpleName() + ")");
                return null;
            }
        }
    }

    /**
     * The class with its one {@code ()PlayerSkin} instance method passing each result
     * through the hook; null when there is not exactly one such method. No branches are
     * added, so the stack map frames stay valid.
     */
    static byte[] hook(byte[] bytes, Type hook) {
        ClassNode node = new ClassNode();
        new ClassReader(bytes).accept(node, 0);
        String getter = "()" + hook.getReturnType().getDescriptor();
        MethodNode found = null;
        for (MethodNode method : node.methods) {
            if ((method.access & Opcodes.ACC_STATIC) == 0 && method.desc.equals(getter)) {
                if (found != null) {
                    return null;
                }
                found = method;
            }
        }
        if (found == null) {
            return null;
        }
        String owner = AgentHooks.class.getName().replace('.', '/');
        for (AbstractInsnNode insn : found.instructions.toArray()) {
            if (insn.getOpcode() == Opcodes.ARETURN) {
                InsnList call = new InsnList();
                call.add(new VarInsnNode(Opcodes.ALOAD, 0));
                call.add(new MethodInsnNode(Opcodes.INVOKESTATIC, owner, "skin", hook.getDescriptor(), false));
                found.instructions.insertBefore(insn, call);
            }
        }
        found.maxStack += 1;
        ClassWriter writer = new ClassWriter(0);
        node.accept(writer);
        return writer.toByteArray();
    }
}
