---- MODULE With ----
EXTENDS TLC

VARIABLES Ready, Flushed, InnerReady, InnerFlushed, HasBuffer

Sink      == INSTANCE Sink
InnerSink == INSTANCE Sink WITH Ready   <- InnerReady,
                                Flushed <- InnerFlushed

TypeOK == /\ Sink!TypeOK
          /\ InnerSink!TypeOK
          /\ HasBuffer \in {TRUE, FALSE}

Init == /\ Sink!Init
        /\ InnerSink!Init
        /\ HasBuffer = FALSE

StartSend == /\ Sink!StartSend
             /\ HasBuffer' = TRUE
             /\ UNCHANGED << InnerReady, InnerFlushed >>

StartSendAssert == StartSend => HasBuffer = FALSE

PollBufferReady == /\ HasBuffer     = TRUE
                   /\ HasBuffer'    = FALSE
                   /\ InnerReady'   = FALSE
                   /\ InnerFlushed' = FALSE
                   /\ UNCHANGED << Ready, Flushed >>

PollBufferReadyAssert == PollBufferReady => InnerSink!StartSend

PollBufferPending == /\ HasBuffer  = TRUE
                     /\ HasBuffer' = TRUE
                     /\ UNCHANGED << Ready, Flushed, InnerReady, InnerFlushed >>

PollBufferPendingAssert == PollBufferPending => (InnerReady = TRUE /\ (Sink!PollReadyPending \/ Sink!PollFlushPending))

PollBufferAssert == (PollBufferReady \/ PollBufferPending) => (Sink!PollReady \/ Sink!PollFlush)

PollReadyReady == /\ HasBuffer   = FALSE
                  /\ InnerReady' = TRUE
                  /\ Ready       = FALSE
                  /\ Ready'      = TRUE
                  /\ UNCHANGED << Flushed, InnerFlushed, HasBuffer >>

PollReadyReadyAssert == PollReadyReady => (InnerSink!PollReadyReady /\ Sink!PollReadyReady)

PollReadyPending == /\ HasBuffer = FALSE
                    /\ Ready     = FALSE
                    /\ Ready'    = FALSE
                    /\ UNCHANGED << Flushed, InnerReady, InnerFlushed, HasBuffer >>

PollReadyPendingAssert ==  PollReadyPending => (InnerSink!PollReadyPending /\ Sink!PollReadyPending)

PollFlushReady == /\ HasBuffer     = FALSE
                  /\ InnerFlushed' = TRUE
                  /\ Flushed       = FALSE
                  /\ Flushed'      = TRUE
                  /\ UNCHANGED << Ready, InnerReady, HasBuffer >>

PollFlushReadyAssert == PollFlushReady => (InnerSink!PollFlushReady /\ Sink!PollFlushReady)

PollFlushPending == /\ HasBuffer = FALSE
                    /\ Flushed   = FALSE
                    /\ Flushed'  = FALSE
                  /\ UNCHANGED << Ready, InnerReady, InnerFlushed, HasBuffer >>

PollFlushPendingAssert == PollFlushPending => (InnerSink!PollFlushPending /\ Sink!PollFlushPending)

CallMethod == \/ StartSend
              \/ PollBufferReady
              \/ PollBufferPending
              \/ PollReadyReady
              \/ PollReadyPending
              \/ PollFlushReady
              \/ PollFlushPending

Next == CallMethod

Spec == Init /\ [][Next]_<< Ready, Flushed, InnerReady, InnerFlushed, HasBuffer >>

FairSpec == /\ Spec
            /\ Sink!EventuallyFlushed

AssertAll == /\ StartSendAssert
             /\ PollBufferReadyAssert
             /\ PollBufferPendingAssert
             /\ PollBufferAssert
             /\ PollReadyReadyAssert
             /\ PollReadyPendingAssert
             /\ PollFlushReadyAssert
             /\ PollFlushPendingAssert

AssertOK == [][AssertAll]_<< Ready, Flushed, InnerReady, InnerFlushed, HasBuffer >>

Complies == /\ Sink!Spec
            /\ InnerSink!Spec

FairComplies == /\ Sink!FairSpec
                /\ InnerSink!FairSpec

====
