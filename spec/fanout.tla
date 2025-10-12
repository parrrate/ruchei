---- MODULE fanout ----
EXTENDS TLC

VARIABLES AReady, AFlushed, BReady, BFlushed, Ready, Flushed

Asink == INSTANCE sink WITH Ready   <- AReady,
                             Flushed <- AFlushed

Bsink == INSTANCE sink WITH Ready   <- BReady,
                             Flushed <- BFlushed

sink  == INSTANCE sink

TypeOK == /\ Asink!TypeOK
          /\ Bsink!TypeOK
          /\  sink!TypeOK

Init == /\ Asink!Init
        /\ Bsink!Init
        /\  sink!Init

AReadiesFirst == /\ AReady  = FALSE
                 /\ BReady  = FALSE
                 /\ AReady' = TRUE
                 /\ UNCHANGED << AFlushed, BFlushed, BReady, Ready, Flushed >>

BReadiesFirst == /\ AReady  = FALSE
                 /\ BReady  = FALSE
                 /\ BReady' = TRUE
                 /\ UNCHANGED << AFlushed, BFlushed, AReady, Ready, Flushed >>

AReadiesLater == /\ AReady  = FALSE
                 /\ BReady  = TRUE
                 /\ AReady' = TRUE
                 /\ Ready'  = TRUE
                 /\ UNCHANGED << AFlushed, BFlushed, BReady, Flushed >>

BReadiesLater == /\ AReady  = TRUE
                 /\ BReady  = FALSE
                 /\ BReady' = TRUE
                 /\ Ready'  = TRUE
                 /\ UNCHANGED << AFlushed, BFlushed, AReady, Flushed >>

AFlushesFirst == /\ AFlushed  = FALSE
                 /\ BFlushed  = FALSE
                 /\ AFlushed' = TRUE
                 /\ UNCHANGED << AReady, BReady, BFlushed, Flushed, Ready >>

BFlushesFirst == /\ AFlushed  = FALSE
                 /\ BFlushed  = FALSE
                 /\ BFlushed' = TRUE
                 /\ UNCHANGED << AReady, BReady, AFlushed, Flushed, Ready >>

AFlushesLater == /\ AFlushed  = FALSE
                 /\ BFlushed  = TRUE
                 /\ AFlushed' = TRUE
                 /\ Flushed'  = TRUE
                 /\ UNCHANGED << AReady, BReady, BFlushed, Ready >>

BFlushesLater == /\ AFlushed  = TRUE
                 /\ BFlushed  = FALSE
                 /\ BFlushed' = TRUE
                 /\ Flushed'  = TRUE
                 /\ UNCHANGED << AReady, BReady, AFlushed, Ready >>

StartSend == /\ Asink!StartSend
             /\ Bsink!StartSend
             /\  sink!StartSend

CallMethod == \/ AReadiesFirst
              \/ BReadiesFirst
              \/ AReadiesLater
              \/ BReadiesLater
              \/ AFlushesFirst
              \/ BFlushesFirst
              \/ AFlushesLater
              \/ BFlushesLater
              \/ StartSend

Next == CallMethod

Spec == Init /\ [][Next]_<< AReady, AFlushed, BReady, BFlushed, Ready, Flushed >>

FairSpec == Spec /\ sink!EventuallyFlushed

Complies == /\  sink!Spec
            /\ Asink!Spec
            /\ Bsink!Spec

FairComplies == /\ Asink!FairSpec
                /\ Bsink!FairSpec

====