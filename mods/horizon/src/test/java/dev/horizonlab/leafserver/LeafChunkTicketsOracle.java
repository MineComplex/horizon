// Writes 26.3's chunk ticket constants for horizon-server's chunk holding tests
// (crates/horizon-server/src/server/chunk_system/oracle_tests.rs). Run with the server owner's own
// 26.3 jar and its libraries on the class path (Java's single-file source launcher):
// `java -cp <jar:libs> LeafChunkTicketsOracle.java <out.txt>`; the file is then given to the ignored
// test `chunk_tickets_match_java` (HORIZON_TICKETS_ORACLE). Original Horizon code.
// Rows:
//   level <FullChunkStatus> <level>            ChunkLevel.byStatus(FullChunkStatus)
//   by_status <chunk status> <level>           ChunkLevel.byStatus(ChunkStatus)
//   full_status <level> <FullChunkStatus>      ChunkLevel.fullStatus(level), levels 0..=50
//   generation_status <level> <status|null>    ChunkLevel.generationStatus(level), levels 0..=50
//   ticket <name> <timeout> <flags>            every vanilla TicketType (Paper's and Moonrise's are
//                                              not in the vanilla jar: they are derived from sources)

package dev.horizonlab.leafserver;

import java.io.PrintWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import net.minecraft.SharedConstants;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.level.ChunkLevel;
import net.minecraft.server.level.FullChunkStatus;
import net.minecraft.server.level.TicketType;
import net.minecraft.world.level.chunk.status.ChunkStatus;

public final class LeafChunkTicketsOracle {
    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        try (PrintWriter out = new PrintWriter(Files.newBufferedWriter(Path.of(args[0])))) {
            // The constants are private in vanilla: read through byStatus(FullChunkStatus).
            for (FullChunkStatus status : FullChunkStatus.values()) {
                out.println("level " + status + " " + ChunkLevel.byStatus(status));
            }
            for (ChunkStatus status : ChunkStatus.getStatusList()) {
                out.println("by_status " + BuiltInRegistries.CHUNK_STATUS.getKey(status).getPath() + " " + ChunkLevel.byStatus(status));
            }
            for (int level = 0; level <= 50; level++) {
                out.println("full_status " + level + " " + ChunkLevel.fullStatus(level));
                ChunkStatus status = ChunkLevel.generationStatus(level);
                out.println("generation_status " + level + " " + (status == null ? "null" : BuiltInRegistries.CHUNK_STATUS.getKey(status).getPath()));
            }
            for (TicketType type : BuiltInRegistries.TICKET_TYPE) {
                out.println("ticket " + BuiltInRegistries.TICKET_TYPE.getKey(type).getPath() + " " + type.timeout() + " " + type.flags());
            }
        }
    }
}
