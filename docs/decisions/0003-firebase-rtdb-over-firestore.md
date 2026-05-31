# ADR 0003: Firebase Realtime Database over Firestore for Streaming Telemetry

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0003 |
| **Title** | Firebase Realtime Database over Firestore for Streaming Telemetry |
| **Status** | Accepted |
| **Date** | 2026-05-31 |

## Context
The ingestion pipeline includes a high-frequency lane (LiveState) that can emit updates near frame cadence. This workload is dominated by rapid overwrite-oriented state propagation rather than ad hoc document analytics. The storage backend must therefore optimize for low-latency, high-throughput JSON updates while preserving cost predictability under continuous streams.  
A decision was needed between Firebase storage models with materially different performance and billing characteristics for write-intensive telemetry traffic.

## Decision
The platform adopts Firebase Realtime Database (RTDB) as the primary cloud store for the v2 schema.

## Rejected Alternatives
- Google Firestore: rejected because document-oriented write billing becomes cost-inefficient under sustained high-frequency telemetry updates.
- Firestore-based adaptation with write coalescing: rejected because coalescing introduces additional buffering complexity and weakens real-time observability guarantees.

## Consequences

### Positive
- Low-latency JSON tree updates aligned with overwrite-heavy telemetry semantics.
- More predictable operating costs for sustained streaming traffic due to bandwidth-oriented economics.

### Negative / Limitations
- Reduced access to Firestore-native advanced querying and indexing features.
- Future analytical workloads may require additional aggregation layers or export pipelines.
