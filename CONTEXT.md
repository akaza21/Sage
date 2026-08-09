# Sage workbench

Sage is a durable desktop workbench for projects, files, and agent-backed
conversations. This language keeps product, protocol, and release discussions
aligned as the application moves toward beta.

## Workbench

**Project**:
A durable Sage workspace containing one or more filesystem roots, its open
resources, and its persistent Chats.
_Avoid_: Workspace, repository

**Resource**:
A file or directory identified inside a Project by its owning root and relative
path.
_Avoid_: Absolute path, loose path

**Chat**:
A persistent conversation that appears as a workbench tab and belongs to one
Project.
_Avoid_: Thread, session

**Turn**:
One user prompt and the bounded agent activity and results produced for it.
_Avoid_: Message, request

**Chat Handoff**:
An explicit user action that sends a previewed, bounded selection of context to
a target Chat as an ordinary prompt.
_Avoid_: Fork, lineage, invisible context transfer

## Harness integration

**Harness**:
An out-of-process agent host selected before a model and connected to Sage
through a supported protocol.
_Avoid_: Model, provider, in-process agent

**Binding**:
The curated bridge that translates one admitted Harness protocol into Sage's
shared conversation model.
_Avoid_: Harness, generic adapter

**Harness Admission**:
Approval of one exact Harness artifact graph and its security, compatibility,
resource, and recovery evidence for use by a Sage build.
_Avoid_: Discovery, installed, available on PATH

## Delivery

**Beta Candidate**:
An exact packaged Sage build that satisfies the recorded beta feature boundary
and acceptance evidence, including at least one admitted Harness.
_Avoid_: Local green build, development checkout
