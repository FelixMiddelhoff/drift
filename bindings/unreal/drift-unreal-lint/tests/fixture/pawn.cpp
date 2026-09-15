// Minimal synthetic stand-in for a UE Actor's Tick chain — not real UE
// headers, just enough shape (a class with a Tick-named method calling
// into a Simulate helper) to exercise reachability scoping without a full
// engine dependency for this tool's own tests.

class ATestPawn
{
public:
    void Tick(float DeltaTime);
    void Simulate(float DeltaTime);
    void NotReached(float DeltaTime);

    float Velocity = 0.0f;
    float Position = 0.0f;
};

void ATestPawn::Tick(float DeltaTime)
{
    Simulate(DeltaTime);
}

// Reachable from Tick via Simulate: a real a+b+c float chain should fire
// exactly once (the whole chain, not once per operator).
void ATestPawn::Simulate(float DeltaTime)
{
    Velocity = Velocity + DeltaTime * 9.8f;
    Position = Position + Velocity + DeltaTime;
}

// Never called from Tick — must not be flagged even though it has the
// same float-chain shape as Simulate.
void ATestPawn::NotReached(float DeltaTime)
{
    Velocity = Velocity + DeltaTime * 9.8f;
}
