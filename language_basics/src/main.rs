fn main() {
    // Variables and Mutability
    // https://doc.rust-lang.org/book/ch03-01-variables-and-mutability.html

    let x = 5;
    println!("The value of x is: {x}");

    // Declaring Constants
    const THREE_HOURS_IN_SECONDS: u32 = 60 * 60 * 3;

    // Shadowing
    let x = x + 1;
    {
        let x = x * 2;
        println!("The value of x in the inner scope is: {x}");
    }

    println!("The value of x is: {x}");
}
