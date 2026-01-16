// @generated automatically by Diesel CLI.

diesel::table! {
    products (id) {
        id -> Integer,
        product_name -> Text,
        product_value -> Integer,
    }
}

diesel::table! {
    taxes (id) {
        id -> Integer,
        state_name -> Text,
        year -> Integer,
        percent -> Integer,
        product_id -> Integer,
    }
}

diesel::joinable!(taxes -> products (product_id));

diesel::allow_tables_to_appear_in_same_query!(products, taxes,);
