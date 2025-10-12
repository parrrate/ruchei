---- MODULE Sink ----
EXTENDS TLC

VARIABLES Ready, Flushed

TypeOK == /\ Ready      \in {TRUE, FALSE}
          /\ Flushed    \in {TRUE, FALSE}

Init == /\ Ready   = FALSE
        /\ Flushed = TRUE

PollReadyReady == /\ Ready = FALSE
                  /\ Ready' = TRUE
                  /\ UNCHANGED << Flushed >>

PollReadyPending == /\ Ready  = FALSE
                    /\ Ready' = FALSE
                    /\ UNCHANGED << Flushed >>

PollReady == \/ PollReadyReady
             \/ PollReadyPending

StartSend == /\ Ready    = TRUE
             /\ Ready'   = FALSE
             /\ Flushed' = FALSE

PollFlushReady == /\ Flushed  = FALSE
                  /\ Flushed' = TRUE
                  /\ UNCHANGED << Ready >>

PollFlushPending == /\ Flushed  = FALSE
                    /\ Flushed' = FALSE
                    /\ UNCHANGED << Ready >>

PollFlush == \/ PollFlushReady
             \/ PollFlushPending

CallMethod == \/ PollReady
              \/ StartSend
              \/ PollFlush

Next == CallMethod

Spec == Init /\ [][Next]_<< Ready, Flushed >>

EventuallyFlushed == WF_<< Ready, Flushed >>(PollFlushReady)

FairSpec == Spec /\ EventuallyFlushed

====
