---- MODULE sink ----
EXTENDS TLC

VARIABLES Ready, Flushed

TypeOK == /\ Ready      \in {TRUE, FALSE}
          /\ Flushed    \in {TRUE, FALSE}

Init == /\ Ready   = FALSE
        /\ Flushed = TRUE

PollReadyReady == Ready' = TRUE

PollReadyPending == /\ Ready  = FALSE
                    /\ Ready' = FALSE

PollReady == /\ \/ PollReadyReady
                \/ PollReadyPending
             /\ UNCHANGED << Flushed >>
             /\ Ready = FALSE

StartSend == /\ Ready    = TRUE
             /\ Ready'   = FALSE
             /\ Flushed' = FALSE

PollFlushReady == Flushed' = TRUE

PollFlushPending == /\ Flushed  = FALSE
                    /\ Flushed' = FALSE

PollFlush == /\ \/ PollFlushReady
                \/ PollFlushPending
             /\ UNCHANGED << Ready >>
             /\ Flushed = FALSE

CallMethod == \/ PollReady
              \/ StartSend
              \/ PollFlush

Next == CallMethod

Spec == Init /\ [][Next]_<< Ready, Flushed >>

GetsFlushed == /\ PollFlush
               /\ PollFlushReady

EventuallyFlushed == []<><<GetsFlushed>>_<< Ready, Flushed >>

FairSpec == Spec /\ EventuallyFlushed

====
