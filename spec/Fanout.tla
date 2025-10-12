---- MODULE Fanout ----
EXTENDS TLC

VARIABLES AReady, AFlushed, BReady, BFlushed, Ready, Flushed

ASink == INSTANCE Sink WITH Ready   <- AReady,
                            Flushed <- AFlushed

BSink == INSTANCE Sink WITH Ready   <- BReady,
                            Flushed <- BFlushed

Sink  == INSTANCE Sink

TypeOK == /\ ASink!TypeOK
          /\ BSink!TypeOK
          /\  Sink!TypeOK

Init == /\ ASink!Init
        /\ BSink!Init
        /\  Sink!Init

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

StartSend == /\ Sink!StartSend
             /\ AReady'   = FALSE
             /\ BReady'   = FALSE
             /\ AFlushed' = FALSE
             /\ BFlushed' = FALSE

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

FairSpec == Spec /\ Sink!EventuallyFlushed

Complies == /\  Sink!Spec
            /\ ASink!Spec
            /\ BSink!Spec

FairComplies == /\ ASink!FairSpec
                /\ BSink!FairSpec

====