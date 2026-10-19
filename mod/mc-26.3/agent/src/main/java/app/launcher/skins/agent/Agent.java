package app.launcher.skins.agent;

import java.lang.classfile.ClassFile;
import java.lang.classfile.ClassTransform;
import java.lang.classfile.Opcode;
import java.lang.classfile.instruction.ReturnInstruction;
import java.lang.constant.ClassDesc;
import java.lang.constant.MethodTypeDesc;
import java.lang.instrument.ClassFileTransformer;
import java.lang.instrument.Instrumentation;
import java.security.ProtectionDomain;

/**
 * Launcher Skins (D-065) for Minecraft 26.3 without a mod loader. The launcher adds
 * {@code -javaagent:} to the game's command line; this rewrites
 * {@code PlayerInfo.getSkin()} so every value it returns passes through
 * {@link AgentHooks#skin}, as the loaders' Mixin does. The game's class names are not
 * obfuscated from 26.1, so nothing needs mapping.
 */
public final class Agent {
    static final String TARGET = "net/minecraft/client/multiplayer/PlayerInfo";
    private static final String SKIN = "Lnet/minecraft/world/entity/player/PlayerSkin;";

    private Agent() {}

    public static void premain(String args, Instrumentation instrumentation) {
        instrumentation.addTransformer(new Transformer());
    }

    static final class Transformer implements ClassFileTransformer {
        @Override
        public byte[] transform(ClassLoader loader, String name, Class<?> redefined,
                ProtectionDomain domain, byte[] bytes) {
            if (!TARGET.equals(name)) {
                return null;
            }
            try {
                return hook(bytes);
            } catch (RuntimeException e) {
                // The game runs unchanged rather than not at all.
                System.err.println("Launcher Skins: PlayerInfo left unchanged (" + e.getClass().getSimpleName() + ")");
                return null;
            }
        }
    }

    /** {@code PlayerInfo} with {@code getSkin()} passing each result through the hook. */
    static byte[] hook(byte[] bytes) {
        ClassFile files = ClassFile.of();
        ClassDesc hooks = ClassDesc.of(AgentHooks.class.getName());
        MethodTypeDesc type = MethodTypeDesc.ofDescriptor("(" + SKIN + "L" + TARGET + ";)" + SKIN);
        return files.transformClass(files.parse(bytes), ClassTransform.transformingMethodBodies(
                method -> method.methodName().equalsString("getSkin")
                        && method.methodType().equalsString("()" + SKIN),
                (code, element) -> {
                    if (element instanceof ReturnInstruction ret && ret.opcode() == Opcode.ARETURN) {
                        code.aload(0);
                        code.invokestatic(hooks, "skin", type);
                    }
                    code.with(element);
                }));
    }
}
