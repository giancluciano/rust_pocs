# Log Builder

A Logger Builder Router System is an architectural pattern for centralized logging where a service routes log events to multiple destinations (like filesystem and ELK stack) based on configurable rules. Here's an overview:

## Core Concept
The system acts as a middleware layer that intercepts logging calls, enriches them with metadata, and intelligently routes them to appropriate destinations based on log level, source, or custom criteria.