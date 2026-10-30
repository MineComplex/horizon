// Writes horizon-server's per-state block data (`block_states.bin`) from the server owner's own 26.3
// server jar. Run by tools/leaf_server_reports.py with the jar and its libraries on the class path
// (Java's single-file source launcher): `java -cp <jar:libs> LeafBlockStatesExporter.java <out>`.
// Original Horizon code: kept with the mod's exporters (tools/check_public_tree.py keeps game-linked
// Java out of tools/).
// Only public accessors of BlockState are read (plus VoxelShape's discrete grid), after
// Bootstrap.bootStrap() has run every state's initCache(). The format is documented in
// crates/horizon-server/src/block/flags.rs; bump VERSION there and here together.

package dev.horizonlab.leafserver;

import it.unimi.dsi.fastutil.doubles.DoubleList;
import java.lang.reflect.Field;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.LiquidBlock;
import net.minecraft.world.level.block.PowderSnowBlock;
import net.minecraft.world.level.block.ScaffoldingBlock;
import net.minecraft.world.level.block.SupportType;
import net.minecraft.world.level.block.piston.MovingPistonBlock;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.NoteBlockInstrument;
import net.minecraft.world.level.material.FlowingFluid;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.material.Fluids;
import net.minecraft.world.level.material.PushReaction;
import net.minecraft.world.phys.shapes.CubePointRange;
import net.minecraft.world.phys.shapes.CubeVoxelShape;
import net.minecraft.world.phys.shapes.DiscreteVoxelShape;
import net.minecraft.world.phys.shapes.Shapes;
import net.minecraft.world.phys.shapes.VoxelShape;

public final class LeafBlockStatesExporter {
    static final int VERSION = 1;
    static final int NO_SHAPE = -1;

    // Per-state flag bits (flags.rs `Flag`).
    static final int AIR = 1, IGNITED_BY_LAVA = 1 << 1, LIQUID = 1 << 2, LEGACY_SOLID = 1 << 3,
        REQUIRES_CORRECT_TOOL = 1 << 4, CAN_OCCLUDE = 1 << 5, SPAWN_TERRAIN_PARTICLES = 1 << 6,
        REPLACEABLE = 1 << 7, RANDOMLY_TICKING = 1 << 8, SOLID_RENDER = 1 << 9,
        PROPAGATES_SKYLIGHT_DOWN = 1 << 10, USE_SHAPE_FOR_LIGHT_OCCLUSION = 1 << 11,
        COLLISION_FULL_BLOCK = 1 << 12, LARGE_COLLISION_SHAPE = 1 << 13, DYNAMIC_SHAPE = 1 << 14,
        HAS_OFFSET = 1 << 15, SIGNAL_SOURCE = 1 << 16, ANALOG_OUTPUT = 1 << 17,
        CONSTANT_COLLISION = 1 << 18, SPECIAL_COLLIDING = 1 << 19, OCCLUDES_FULL_BLOCK = 1 << 20;

    static Field discreteField;
    static final Map<ByteBuffer, Integer> shapeIds = new HashMap<>();
    static final List<byte[]> shapes = new ArrayList<>();

    /** A shape, little-endian: kind 0 `Shapes.empty()`, 1 `Shapes.block()`, 2 a grid. */
    static byte[] encodeShape(VoxelShape shape) throws Exception {
        if (shape == Shapes.empty() || shape == Shapes.block()) {
            return new byte[] {(byte) (shape == Shapes.empty() ? 0 : 1)};
        }
        var out = ByteBuffer.allocate(1 << 16).order(ByteOrder.LITTLE_ENDIAN);
        out.put((byte) 2).put((byte) (shape instanceof CubeVoxelShape ? 1 : 0));
        int[] sizes = new int[3];
        for (Direction.Axis axis : Direction.Axis.values()) {
            DoubleList coords = shape.getCoords(axis);
            sizes[axis.ordinal()] = coords.size() - 1;
            boolean cube = coords instanceof CubePointRange;
            out.put((byte) (cube ? 1 : 0)).putShort((short) (cube ? coords.size() - 1 : coords.size()));
            for (int i = 0; !cube && i < coords.size(); i++) {
                out.putDouble(coords.getDouble(i));
            }
        }
        var discrete = (DiscreteVoxelShape) discreteField.get(shape);
        byte[] bits = new byte[(sizes[0] * sizes[1] * sizes[2] + 7) / 8];
        for (int x = 0, i = 0; x < sizes[0]; x++) {
            for (int y = 0; y < sizes[1]; y++) {
                for (int z = 0; z < sizes[2]; z++, i++) {
                    if (discrete.isFull(x, y, z)) {
                        bits[i / 8] |= (byte) (1 << (i % 8));
                    }
                }
            }
        }
        out.put(bits);
        return java.util.Arrays.copyOf(out.array(), out.position());
    }

    /** The shape's index in the de-duplicated list (equal grids share one entry). */
    static int shapeId(VoxelShape shape) throws Exception {
        byte[] encoded = encodeShape(shape);
        return shapeIds.computeIfAbsent(ByteBuffer.wrap(encoded), k -> {
            shapes.add(encoded);
            return shapes.size() - 1;
        });
    }

    static void names(ByteBuffer out, List<String> names) {
        out.putShort((short) names.size());
        for (String name : names) {
            byte[] utf8 = name.getBytes(java.nio.charset.StandardCharsets.UTF_8);
            out.put((byte) utf8.length).put(utf8);
        }
    }

    /** Leaf's `optimize-collision-shape` patch: a state's collision shape ignores the context. */
    static boolean constantCollision(BlockState state) {
        Block block = state.getBlock();
        if (block instanceof LiquidBlock) {
            return state.getFluidState().is(Fluids.WATER);
        }
        return !(block instanceof ScaffoldingBlock) && !(block instanceof PowderSnowBlock);
    }

    @SuppressWarnings("deprecation")
    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        discreteField = VoxelShape.class.getDeclaredField("shape");
        discreteField.setAccessible(true);
        var level = EmptyBlockGetter.INSTANCE;
        var pos = BlockPos.ZERO;
        int count = Block.BLOCK_STATE_REGISTRY.size();
        var records = ByteBuffer.allocate(count * 28).order(ByteOrder.LITTLE_ENDIAN);
        for (int id = 0; id < count; id++) {
            BlockState state = Block.BLOCK_STATE_REGISTRY.byId(id);
            boolean dynamic = state.getBlock().hasDynamicShape();
            int flags = 0;
            if (state.isAir()) flags |= AIR;
            if (state.ignitedByLava()) flags |= IGNITED_BY_LAVA;
            if (state.liquid()) flags |= LIQUID;
            if (state.isSolid()) flags |= LEGACY_SOLID;
            if (state.requiresCorrectToolForDrops()) flags |= REQUIRES_CORRECT_TOOL;
            if (state.canOcclude()) flags |= CAN_OCCLUDE;
            if (state.shouldSpawnTerrainParticles()) flags |= SPAWN_TERRAIN_PARTICLES;
            if (state.canBeReplaced()) flags |= REPLACEABLE;
            if (state.isRandomlyTicking()) flags |= RANDOMLY_TICKING;
            if (state.isSolidRender()) flags |= SOLID_RENDER;
            if (state.propagatesSkylightDown()) flags |= PROPAGATES_SKYLIGHT_DOWN;
            if (state.useShapeForLightOcclusion()) flags |= USE_SHAPE_FOR_LIGHT_OCCLUSION;
            if (state.hasOffsetFunction()) flags |= HAS_OFFSET;
            if (state.isSignalSource()) flags |= SIGNAL_SOURCE;
            if (state.hasAnalogOutputSignal()) flags |= ANALOG_OUTPUT;
            int sturdy = 0;
            int collision = NO_SHAPE;
            if (dynamic) {
                flags |= DYNAMIC_SHAPE;
            } else {
                // The Cache's values (initCache built it: the block has no dynamic shape).
                VoxelShape shape = state.getCollisionShape(level, pos);
                collision = shapeId(shape);
                if (state.isCollisionShapeFullBlock(level, pos)) flags |= COLLISION_FULL_BLOCK;
                if (state.hasLargeCollisionShape()) flags |= LARGE_COLLISION_SHAPE;
                if (constantCollision(state)) flags |= CONSTANT_COLLISION;
                // Moonrise's occludesFullBlock: the shape covers the whole unit cube.
                if (!shape.isEmpty() && !Shapes.joinIsNotEmpty(Shapes.block(), shape,
                        net.minecraft.world.phys.shapes.BooleanOp.ONLY_FIRST)) {
                    flags |= OCCLUDES_FULL_BLOCK;
                }
                for (Direction d : Direction.values()) {
                    for (SupportType type : SupportType.values()) {
                        if (state.isFaceSturdy(level, pos, d, type)) {
                            sturdy |= 1 << (d.ordinal() * SupportType.values().length + type.ordinal());
                        }
                    }
                }
            }
            if (state.hasLargeCollisionShape() || state.getBlock() instanceof MovingPistonBlock) {
                flags |= SPECIAL_COLLIDING;
            }
            FluidState fluid = state.getFluidState();
            int fluidByte = BuiltInRegistries.FLUID.getId(fluid.getType())
                | (fluid.isEmpty() ? 0 : fluid.getAmount()) << 3
                | (fluid.hasProperty(FlowingFluid.FALLING) && fluid.getValue(FlowingFluid.FALLING) ? 0x80 : 0);
            records.putInt(flags).putInt(sturdy)
                .put((byte) state.getLightEmission()).put((byte) state.getLightDampening())
                .put((byte) state.getPistonPushReaction().ordinal())
                .put((byte) state.getMapColor(level, pos).id)
                .put((byte) state.instrument().ordinal()).put((byte) fluidByte).putShort((short) 0)
                .putFloat(state.getDestroySpeed(level, pos))
                .putInt(collision).putInt(shapeId(state.getOcclusionShape()));
        }

        var header = ByteBuffer.allocate(1 << 16).order(ByteOrder.LITTLE_ENDIAN);
        header.put("HZBS".getBytes()).putInt(VERSION).putInt(count);
        var fluids = new ArrayList<String>();
        BuiltInRegistries.FLUID.forEach(f -> fluids.add(BuiltInRegistries.FLUID.getKey(f).toString()));
        names(header, fluids);
        var push = new ArrayList<String>();
        for (PushReaction r : PushReaction.values()) push.add(r.name());
        names(header, push);
        var instruments = new ArrayList<String>();
        for (NoteBlockInstrument i : NoteBlockInstrument.values()) instruments.add(i.getSerializedName());
        names(header, instruments);
        header.putInt(shapes.size());

        try (var file = Files.newOutputStream(Path.of(args[0]))) {
            file.write(header.array(), 0, header.position());
            for (byte[] shape : shapes) {
                file.write(shape);
            }
            file.write(records.array());
        }
        System.out.println("wrote " + count + " block states, " + shapes.size() + " shapes to " + args[0]);
    }
}
