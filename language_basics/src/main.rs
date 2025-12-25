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

    // control flow in rust

    let number = 3;

    if number < 5 {
        println!("condition is true");
    } else {
        println!("condition is false");
    }

    // infinite loops

    // loop {
    //     println!("again!")
    // }

    let mut counter = 0;

    let result = loop {
        counter += 1;
        if counter == 10 {
            break counter * 2;
        }
    };

    println!("the result is {result}");

    // labeling loops

    let mut count = 0;

    'external_loop: loop {
        let mut remaining = 10;
        loop {
            if remaining == 9 {
                break;
            }
            if count == 2 {
                break 'external_loop;
            }
            remaining -= 1;
        }
        count += 1;
    }
    println!("end count = {count}");

    // while loops
    let mut number = 3;

    while number != 0 {
        println!("{number}!");

        number -= 1;
    }

    println!("LIFTOFF!!!");

    // and for loops as well
    let a = [10, 20, 30, 40, 50];

    for element in a {
        println!("the value is: {element}");
    }

    for number in (1..4).rev() {
        println!("{number}!");
    }
    println!("LIFTOFF!!!");
}
