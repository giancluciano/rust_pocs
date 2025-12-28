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

    // functions
    fn new_function() {
        println!("new function");
    }

    new_function();

    fn another_function(x: i32) {
        println!("x is: {x}");
    }
    another_function(5);

    let y = {
        let x = 2;
        x + 1
    };
    println!("value of y is: {y}");

    // patterns
    // destruction

    let triple = (-1, 0, 1);

    match triple {
        (0, y, z) => println!("first one is zero, second is {y}, third is {z}"),
        (1, ..) => println!("first one is 1"),
        (.., 2) => println!("last one is two"),
        (-1, .., 1) => println!("first is -1, last is 1"),
        _ => println!("else")
    }

    let array = [-1, 0, 1];
    
    match array {
        [-1, second, third] => println!("second value is = {}, third is = {}", second, third),
        _ => println!("else")
    }

    let number = 13;
    match number {
        1 => println!("one"),
        2..=10 => println!("ten or less"),
        12 | 13 => println!("12 or 13"),
        _ => println!("the rest")
    }
}
