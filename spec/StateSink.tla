---- MODULE StateSink ----
EXTENDS TLC

VARIABLES Ready, Flushed, State

Sink == INSTANCE Sink

TypeOK == /\ Sink!TypeOK
          /\ State \in {"readying", "ready", "started", "flushing", "flushed"}

Init == /\ Sink!Init
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

Complies == Sink!Spec

FairComplies == Sink!FairSpec

====
