# ADR 0011: Schema Normalization at the Ingress Boundary

## Metadata

| Field | Value |
|-------|-------|
| **ADR** | 0011 |
| **Title** | Schema Normalization at the Ingress Boundary |
| **Status** | Accepted |
| **Date** | 2026-05-31 |

## Context
The telemetry source ecosystem evolves over time, with key naming conventions appearing in both legacy snake_case and newer camelCase forms. Without early normalization, schema drift propagates downstream into storage, analytics, and UI contracts, producing fragmented datasets and brittle consumers. In a streaming architecture, contract inconsistency is multiplicative: every downstream lane and sink inherits upstream ambiguity.  
A boundary decision was required to stabilize the internal data model and protect system-wide interoperability.

## Decision
Ingress transformation explicitly detects and maps both snake_case and camelCase payload variants into a unified internal schema before routing and persistence.

## Rejected Alternatives
- Passing raw JSON directly to cloud storage: rejected because heterogeneous key shapes create unqueryable, inconsistent datasets.
- Deferring normalization to downstream consumers: rejected because it duplicates parsing logic, increases defect surface, and weakens contract governance.
- Enforcing strict single-format ingestion with hard rejection: rejected because it reduces resilience to real-world upstream version skew.

## Consequences

### Positive
- Establishes a stable internal contract for routing, persistence, and frontend consumption.
- Contains schema drift at the system boundary, reducing downstream complexity and analytics ambiguity.

### Negative / Limitations
- Requires ongoing maintenance of explicit mapping rules as upstream payloads evolve.
- Introduces additional CPU work in the ingestion path due to normalization logic.
