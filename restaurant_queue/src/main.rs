use std::collections::VecDeque;
use std::time::Duration;

struct Dish {
    name: String,
    prep_time: Duration,
}

struct Order {
    id: u32,
    customer: String,
    dish: Dish,
}

struct RestaurantQueue {
    orders: VecDeque<Order>,
    next_id: u32,
}

impl RestaurantQueue {
    fn new() -> Self {
        RestaurantQueue {
            orders: VecDeque::new(),
            next_id: 1,
        }
    }

    fn add_order(&mut self, customer: &str, dish: Dish) -> (u32, Duration) {
        let id = self.next_id;
        self.next_id += 1;

        let eta = self.orders.iter().map(|o| o.dish.prep_time).sum::<Duration>() + dish.prep_time;

        self.orders.push_back(Order {
            id,
            customer: customer.to_string(),
            dish,
        });

        (id, eta)
    }

    #[allow(dead_code)]
    fn estimated_wait_for(&self, order_id: u32) -> Option<Duration> {
        let mut total = Duration::ZERO;
        for order in &self.orders {
            total += order.dish.prep_time;
            if order.id == order_id {
                return Some(total);
            }
        }
        None
    }

    fn serve_next(&mut self) -> Option<Order> {
        self.orders.pop_front()
    }

    fn print_status(&self) {
        if self.orders.is_empty() {
            println!("Queue is empty.");
            return;
        }

        println!("{:<5} {:<15} {:<15} {:<10}", "Pos", "Customer", "Dish", "ETA (min)");
        println!("{}", "-".repeat(50));

        let mut cumulative = Duration::ZERO;
        for (i, order) in self.orders.iter().enumerate() {
            cumulative += order.dish.prep_time;
            println!(
                "{:<5} {:<15} {:<15} {:<10}",
                i + 1,
                order.customer,
                order.dish.name,
                cumulative.as_secs() / 60
            );
        }
    }
}

fn main() {
    let dishes = [
        ("Pasta",  12u64),
        ("Burger",  8),
        ("Salad",   3),
        ("Steak",  20),
        ("Soup",    5),
    ];

    let make_dish = |(name, mins): &(&str, u64)| Dish {
        name: name.to_string(),
        prep_time: Duration::from_secs(mins * 60),
    };

    let mut queue = RestaurantQueue::new();

    let orders_to_add = [
        ("Alice",   &dishes[0]),
        ("Bob",     &dishes[1]),
        ("Carol",   &dishes[2]),
        ("Dave",    &dishes[3]),
        ("Eve",     &dishes[4]),
        ("Frank",   &dishes[0]),
    ];

    println!("=== Adding orders ===");
    for (customer, dish_spec) in &orders_to_add {
        let (id, eta) = queue.add_order(customer, make_dish(dish_spec));
        println!(
            "Order #{id} for {customer} ({dish}) — ETA: {} min",
            eta.as_secs() / 60,
            dish = dish_spec.0
        );
    }

    println!("\n queue status");
    queue.print_status();

    println!("\n Serving");
    for _ in 0..2 {
        if let Some(order) = queue.serve_next() {
            println!("Served: {} — {} (Order #{})", order.customer, order.dish.name, order.id);
        }
    }

    println!("\n updating ");
    queue.print_status();
}
