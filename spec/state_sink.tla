---- MODULE state_sink ----
EXTENDS TLC

VARIABLES Ready, Flushed, State

sink == INSTANCE sink

TypeOK == /\ sink!TypeOK
          /\ State \in {"readying", "ready", "started", "flushing", "flushed"}

Init == /\ sink!Init
        /\ State = "flushed"

StartedToReadying == /\ State    = "started"
                     /\ Ready'   = FALSE
                     /\ Flushed' = FALSE
                     /\ State'   = "readying"

FlushedToReadying == /\ State    = "flushed"
                     /\ Ready'   = FALSE
                     /\ Flushed' = TRUE
                     /\ State'   = "readying"

ReadyingToReady   == /\ State    = "readying"
                     /\ Ready'   = TRUE
                     /\ Flushed' = Flushed
                     /\ State'   = "ready"

ReadyToStarted    == /\ State    = "ready"
                     /\ Ready'   = FALSE
                     /\ Flushed' = FALSE
                     /\ State'   = "started"

StartedToFlushing == /\ State    = "started"
                     /\ Ready'   = FALSE
                     /\ Flushed' = FALSE
                     /\ State'   = "flushing"

FlushingToFlushed == /\ State    = "flushing"
                     /\ Ready'   = FALSE
                     /\ Flushed' = TRUE
                     /\ State'   = "flushed"

Next == \/ StartedToReadying
        \/ FlushedToReadying
        \/ ReadyingToReady
        \/ ReadyToStarted
        \/ StartedToFlushing
        \/ FlushingToFlushed

Spec == Init /\ [][Next]_<< Ready, Flushed, State >>

FairSpec == Spec /\ []<><<FlushingToFlushed>>_<< Ready, Flushed, State >>

Complies == sink!Spec

FairComplies == sink!FairSpec

====
